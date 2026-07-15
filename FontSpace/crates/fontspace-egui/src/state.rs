//! Application workspace state: the open FontSpace document(s) the GUI edits, the
//! active document's selection, and editor view options.
//!
//! The workspace holds its documents as [`OpenDocument`]s ([`crate::workspace`]): one
//! **active** document plus a background of the others. The views read the active one
//! through an active-document facade ([`AppState::document`], [`AppState::selection`],
//! …); opening a file adds a document and switching swaps which is active (spec/11).
//! The GUI owns this state but **not** font semantics — mutations go through
//! `fontspace-ops` (spec/02).

use std::path::{Path, PathBuf};

use fontspace_json::LoadOutcome;
use fontspace_model::{
    Bitmap, CharacterEntry, CharacterSet, CharacterSetId, FontSpace, Glyph, GlyphPage, GlyphSet,
    GlyphSetId, GlyphSize, Guide, GuideAxis, GuideId, IdGen, PageId, RandomIdGen,
};

use fontspace_ops::{
    AddGuide, ChangeSet, FontSpaceWarning, GlyphRef, MoveGuide, RemoveCharacterEntry, RemoveGuide,
    SetGuideVisible, SetPixels, add_guide, apply_change_set, move_guide, remove_character_entry,
    remove_guide, set_guide_visible, set_pixels, undo,
};

use crate::editor::geometry::GridLevel;
use crate::editor::stroke::Stroke;
use crate::workspace::{DocumentId, OpenDocument};

/// What the editor and inspector are currently pointed at: one glyph, identified by
/// its glyph set, page, and character `code` (spec/12 §12.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub glyph_set_id: GlyphSetId,
    pub page_id: PageId,
    pub code: u32,
}

/// A file action that would discard the active document's unsaved edits, held pending
/// confirmation (spec/12 §12.12). The guard prevents a click from silently discarding
/// edits; the user confirms or cancels. (Open no longer discards — it opens a new
/// document — so it is unguarded; Close joins this in a later slice.)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuardedIntent {
    /// Reload the active document from its file on disk, discarding its edits.
    Revert,
}

/// The editable state of the workspace: the open documents plus view options.
///
/// Each document's content, path, dirty flag, and selection live on its
/// [`OpenDocument`]; the views reach the active one through the facade accessors below,
/// so they never index the document list. The dirty flag (spec/11 §11.2) is
/// conservative: it may read true after undoing back to the saved state, which only
/// ever asks for an unneeded confirm, never risks silent data loss.
pub struct AppState {
    /// The active open document — the one the views render and edit (spec/11 §11.1).
    active: OpenDocument,
    /// The other open documents, most-recently-active first. Opening a file makes it
    /// active and pushes the previous active here; switching swaps one back to active.
    background: Vec<OpenDocument>,
    /// Injected id source for object-creating operations (spec/03 §Id injection).
    pub ids: Box<dyn IdGen>,
    pub grid: GridLevel,
    /// Editable sample text for the text-preview view (spec/12 §12.10). UI state,
    /// not document data — it is never written to the `.fontspace.json`.
    pub preview_text: String,
    /// Whether the text preview draws 1px dividers between glyph cells (spec/12
    /// §12.10). UI state, default off.
    pub preview_dividers: bool,
    /// A document-replacing action awaiting unsaved-changes confirmation (spec/12
    /// §12.12), or `None` when no modal is open.
    pending_discard: Option<GuardedIntent>,
    /// The most recent save/open result or error, shown in the status strip. It
    /// persists until the next file action replaces it (the dirty marker, not this
    /// line, is the authoritative unsaved-state signal).
    status: Option<String>,
    /// The stroke currently being dragged in the editor, if any (spec/12 §12.4).
    active_stroke: Option<Stroke>,
    /// Workspace-level undo/redo stacks (spec/07 §7.7, spec/11 §11.6): a single stack
    /// across all open documents. Each entry is tagged with the [`DocumentId`] it
    /// applies to, so undo/redo targets the originating document even after the user
    /// switches which one is active.
    undo_stack: Vec<(DocumentId, ChangeSet)>,
    redo_stack: Vec<(DocumentId, ChangeSet)>,
    /// A character-set entry the user has asked to remove, awaiting confirmation of
    /// its cascade impact (spec/12 §12.7). Stored as a resolved `(set, code)` target
    /// so a later selection change can't retarget the confirm.
    pending_remove: Option<(CharacterSetId, u32)>,
}

impl Default for AppState {
    fn default() -> Self {
        Self::with_ids(Box::new(RandomIdGen))
    }
}

impl AppState {
    /// Builds the starter document with the given id source (production wires
    /// [`RandomIdGen`]; tests can wire a `SequentialIdGen` for reproducibility).
    pub fn with_ids(mut ids: Box<dyn IdGen>) -> Self {
        let (content, selection) = starter_document(ids.as_mut());
        // Mint the runtime-only DocumentId after the content so the document's own
        // object ids keep their sequential positions (tests stay reproducible).
        let active = OpenDocument::new(DocumentId::new(ids.as_mut()), content, selection);
        Self {
            active,
            background: Vec::new(),
            ids,
            grid: GridLevel::Subtle,
            preview_text: "AAA HAH".to_string(),
            preview_dividers: false,
            pending_discard: None,
            status: None,
            active_stroke: None,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            pending_remove: None,
        }
    }

    /// The active document's content — what the views render and edit (the facade over
    /// the workspace; generalizing to several documents keeps this signature).
    pub fn document(&self) -> &FontSpace {
        &self.active.content
    }

    /// The active document's current selection (spec/12 §12.3).
    pub fn selection(&self) -> Selection {
        self.active.selection
    }

    /// Points the selection at `code` within the current glyph set and page (e.g.
    /// from clicking a page-overview thumbnail).
    pub fn select_code(&mut self, code: u32) {
        self.active.selection.code = code;
    }

    /// Points the selection at a `(glyph_set, page)` — e.g. from clicking a page in the
    /// document browser (spec/12 §12.2). **Switching glyph set keeps the current code
    /// point** whenever the target's character set has it, so navigating pages/sets
    /// doesn't lose the user's place; only when the code isn't available there does it
    /// fall back to the first character-set entry, else the first stored glyph, else
    /// the current code.
    pub fn select_page(&mut self, glyph_set_id: GlyphSetId, page_id: PageId) {
        let current = self.active.selection.code;
        let code = self
            .active
            .content
            .glyph_set(glyph_set_id)
            .map(|glyph_set| {
                let character_set = self
                    .active
                    .content
                    .character_set(glyph_set.character_set_id);
                if character_set.is_some_and(|cs| cs.contains_code(current)) {
                    current // keep the code point across the switch
                } else {
                    character_set
                        .and_then(|cs| cs.entries.first().map(|entry| entry.code))
                        .or_else(|| {
                            glyph_set
                                .page_of_id(page_id)
                                .and_then(|page| page.glyphs.first().map(|glyph| glyph.code))
                        })
                        .unwrap_or(current)
                }
            })
            .unwrap_or(current);
        self.active.selection = Selection {
            glyph_set_id,
            page_id,
            code,
        };
    }

    /// The current value of pixel `(x, y)` on the selected glyph (`false` if the
    /// glyph is absent/blank or the coordinate is out of bounds).
    pub fn selected_pixel(&self, x: u16, y: u16) -> bool {
        self.selected_bitmap()
            .is_some_and(|bitmap| bitmap.get(x, y).unwrap_or(false))
    }

    /// Begins an editor stroke at `cell`. Per spec/12 §12.4 the first pixel fixes the
    /// mode: starting on an off pixel paints on; on an on pixel, erases.
    pub fn begin_stroke(&mut self, cell: (u16, u16)) {
        let mut stroke = Stroke::begin(!self.selected_pixel(cell.0, cell.1));
        stroke.extend_to(cell);
        self.active_stroke = Some(stroke);
    }

    /// Extends the active stroke to `cell` (no-op if no stroke is active).
    pub fn extend_stroke(&mut self, cell: (u16, u16)) {
        if let Some(stroke) = &mut self.active_stroke {
            stroke.extend_to(cell);
        }
    }

    /// The active stroke, for the editor's live tentative preview.
    pub fn active_stroke(&self) -> Option<&Stroke> {
        self.active_stroke.as_ref()
    }

    /// Commits the active stroke as one `SetPixels` command — one undo entry
    /// (spec/07 §7.4, §7.7). A stroke that changes nothing records nothing. Committing
    /// a new change discards the redo stack.
    pub fn commit_stroke(&mut self) {
        let Some(stroke) = self.active_stroke.take() else {
            return;
        };
        if stroke.is_empty() {
            return;
        }
        let request = SetPixels {
            target: GlyphRef {
                glyph_set_id: self.active.selection.glyph_set_id,
                page_id: self.active.selection.page_id,
                code: self.active.selection.code,
            },
            edits: stroke.edits(),
        };
        // The selection resolves and cells are in-bounds, so this does not fail; a
        // stroke that paints pixels to their current value yields an empty change set.
        if let Ok(change_set) = set_pixels(&mut self.active.content, &request) {
            self.record(change_set);
        }
    }

    /// Records a committed change to the **active** document on the undo stack
    /// (clearing redo) and marks it dirty, skipping an empty (no-op) change set. The
    /// single place edits enter the history — so it is also the single place the dirty
    /// flag is raised. The entry is tagged with the active document's id so undo/redo
    /// stays correct after switching documents (spec/11 §11.6).
    fn record(&mut self, change_set: ChangeSet) {
        if !change_set.is_empty() {
            self.undo_stack.push((self.active.id, change_set));
            self.redo_stack.clear();
            self.active.dirty = true;
        }
    }

    /// Adds a guide of `axis` to the selected page as one undo entry (spec/12 §12.6).
    pub fn add_guide(&mut self, axis: GuideAxis) {
        let name = match axis {
            GuideAxis::Horizontal => "h-guide",
            GuideAxis::Vertical => "v-guide",
        };
        let request = AddGuide {
            glyph_set_id: self.active.selection.glyph_set_id,
            page_id: self.active.selection.page_id,
            name: name.to_string(),
            axis,
            position: 0,
            visible: true,
            locked: false,
        };
        if let Ok(change_set) = add_guide(&mut self.active.content, &request, self.ids.as_mut()) {
            self.record(change_set);
        }
    }

    /// Removes a guide from the selected page as one undo entry (spec/12 §12.6).
    pub fn remove_guide(&mut self, guide_id: GuideId) {
        let request = RemoveGuide {
            glyph_set_id: self.active.selection.glyph_set_id,
            page_id: self.active.selection.page_id,
            guide_id,
        };
        if let Ok(change_set) = remove_guide(&mut self.active.content, &request) {
            self.record(change_set);
        }
    }

    /// Shows/hides a guide as one undo entry (spec/12 §12.6).
    pub fn set_guide_visible(&mut self, guide_id: GuideId, visible: bool) {
        let request = SetGuideVisible {
            glyph_set_id: self.active.selection.glyph_set_id,
            page_id: self.active.selection.page_id,
            guide_id,
            visible,
        };
        if let Ok(change_set) = set_guide_visible(&mut self.active.content, &request) {
            self.record(change_set);
        }
    }

    /// Moves a guide to `position` (a grid-line coordinate) as one undo entry.
    pub fn move_guide(&mut self, guide_id: GuideId, position: i32) {
        let request = MoveGuide {
            glyph_set_id: self.active.selection.glyph_set_id,
            page_id: self.active.selection.page_id,
            guide_id,
            position,
        };
        if let Ok(change_set) = move_guide(&mut self.active.content, &request) {
            self.record(change_set);
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    /// Undoes the most recent committed change, on the document it came from — even if
    /// that isn't the active one (spec/07 §7.7, spec/11 §11.6).
    pub fn undo(&mut self) {
        if let Some((doc_id, change_set)) = self.undo_stack.pop() {
            if let Some(document) = self.document_mut_by_id(doc_id) {
                // The change set came from this document, so its inverse applies cleanly.
                let _ = undo(&mut document.content, &change_set);
                document.dirty = true;
            }
            self.redo_stack.push((doc_id, change_set));
        }
    }

    /// Redoes the most recently undone change, on its originating document (spec/07
    /// §7.7, spec/11 §11.6).
    pub fn redo(&mut self) {
        if let Some((doc_id, change_set)) = self.redo_stack.pop() {
            if let Some(document) = self.document_mut_by_id(doc_id) {
                let _ = apply_change_set(&mut document.content, &change_set);
                document.dirty = true;
            }
            self.undo_stack.push((doc_id, change_set));
        }
    }

    /// The open document with `id` (active or background), if present.
    fn document_mut_by_id(&mut self, id: DocumentId) -> Option<&mut OpenDocument> {
        if self.active.id == id {
            Some(&mut self.active)
        } else {
            self.background
                .iter_mut()
                .find(|document| document.id == id)
        }
    }

    /// The selected glyph set and page, if the selection still resolves.
    pub fn selected_context(&self) -> Option<(&GlyphSet, &GlyphPage)> {
        let glyph_set = self
            .active
            .content
            .glyph_set(self.active.selection.glyph_set_id)?;
        let page = glyph_set.page_of_id(self.active.selection.page_id)?;
        Some((glyph_set, page))
    }

    /// The bitmap of the selected glyph, if one is stored (absent → blank in the UI).
    pub fn selected_bitmap(&self) -> Option<&Bitmap> {
        let (_, page) = self.selected_context()?;
        page.glyph_of_code(self.active.selection.code)
            .map(|g| &g.bitmap)
    }

    /// The label of the selected code in the glyph set's character set, if any.
    pub fn selected_label(&self) -> Option<&str> {
        let glyph_set = self
            .active
            .content
            .glyph_set(self.active.selection.glyph_set_id)?;
        let character_set = self
            .active
            .content
            .character_set(glyph_set.character_set_id)?;
        character_set
            .entry(self.active.selection.code)
            .map(|entry| entry.label.as_str())
    }

    /// The character set referenced by the selected glyph set, if it resolves.
    pub fn selected_character_set(&self) -> Option<&CharacterSet> {
        let glyph_set = self
            .active
            .content
            .glyph_set(self.active.selection.glyph_set_id)?;
        self.active
            .content
            .character_set(glyph_set.character_set_id)
    }

    fn selected_character_set_id(&self) -> Option<CharacterSetId> {
        Some(
            self.active
                .content
                .glyph_set(self.active.selection.glyph_set_id)?
                .character_set_id,
        )
    }

    /// How many glyphs removing `code` from character set `character_set_id` would
    /// cascade-delete (spec/07 §7.8). Computed by dry-running the op on a clone, so
    /// the real document is untouched.
    fn remove_cascade_count(&self, character_set_id: CharacterSetId, code: u32) -> usize {
        let mut preview = self.active.content.clone();
        let Ok(change_set) = remove_character_entry(
            &mut preview,
            &RemoveCharacterEntry {
                character_set_id,
                code,
            },
        ) else {
            return 0;
        };
        change_set
            .warnings
            .iter()
            .map(|warning| match warning {
                FontSpaceWarning::RemoveCascade { removed, .. } => removed.len(),
                _ => 0,
            })
            .sum()
    }

    /// How many glyphs removing `code` from the *selected* character set would
    /// cascade-delete (spec/12 §12.7 pre-apply impact).
    pub fn preview_remove_cascade(&self, code: u32) -> usize {
        self.selected_character_set_id()
            .map_or(0, |id| self.remove_cascade_count(id, code))
    }

    /// The entry code awaiting remove-confirmation, if any (spec/12 §12.7).
    pub fn pending_remove(&self) -> Option<u32> {
        self.pending_remove.map(|(_, code)| code)
    }

    /// The pending remove's `(code, cascade_count)`, computed against the character
    /// set resolved when the remove was armed — so the impact shown always matches
    /// what a confirm will delete, even if the selection later moves.
    pub fn pending_remove_impact(&self) -> Option<(u32, usize)> {
        self.pending_remove.map(|(character_set_id, code)| {
            (code, self.remove_cascade_count(character_set_id, code))
        })
    }

    /// Asks to remove `code`, arming the impact confirmation. Resolves the target
    /// character set now (CLAUDE.md: resolve to a concrete target before mutating),
    /// so a later selection change can't retarget the confirm.
    pub fn request_remove(&mut self, code: u32) {
        self.pending_remove = self.selected_character_set_id().map(|id| (id, code));
    }

    /// Dismisses a pending remove without applying it.
    pub fn cancel_remove(&mut self) {
        self.pending_remove = None;
    }

    /// Applies the pending remove (if any) against its armed target, clearing the
    /// confirmation. Cascade-deletes the entry's glyphs as one undo entry (spec/07
    /// §7.8). Removing an entry with no glyphs still records the entry removal.
    pub fn confirm_remove(&mut self) {
        let Some((character_set_id, code)) = self.pending_remove.take() else {
            return;
        };
        if let Ok(change_set) = remove_character_entry(
            &mut self.active.content,
            &RemoveCharacterEntry {
                character_set_id,
                code,
            },
        ) {
            self.record(change_set);
        }
    }

    // --- Persistence: path binding, dirty tracking, and the unsaved-changes guard
    // (spec/12 §12.12, spec/11 §11.2). File dialogs and the actual atomic read/write
    // live in the app shell (`fontspace_json::{read_document, write_document}`); this
    // state only models what those actions do to the in-memory document. ---

    /// Whether the document has unsaved edits.
    pub fn is_dirty(&self) -> bool {
        self.active.dirty
    }

    /// The file the document is bound to, if it has been saved/opened.
    pub fn path(&self) -> Option<&Path> {
        self.active.path.as_deref()
    }

    /// The active document's display name: its file name, or `"Untitled"` if never
    /// saved.
    pub fn document_name(&self) -> String {
        self.active.display_name()
    }

    /// The status message (last save/open outcome or error), if any.
    pub fn status(&self) -> Option<&str> {
        self.status.as_deref()
    }

    /// Sets the status strip to an error message (e.g. a failed save/open).
    pub fn set_error(&mut self, message: impl Into<String>) {
        self.status = Some(message.into());
    }

    /// Records that the document was just written to `path`: binds the file and clears
    /// the dirty flag (spec/12 §12.12).
    pub fn mark_saved(&mut self, path: PathBuf) {
        self.status = Some(format!("Saved {}", display_name(&path)));
        self.active.path = Some(path);
        self.active.dirty = false;
    }

    /// Reloads the **active** document in place from `path` (Revert): replaces its
    /// content, resets its selection, drops *its* undo/redo history (other documents'
    /// entries stay), clears any in-progress interaction, and marks it clean. Load
    /// warnings (e.g. dangling glyphs) go to the status strip.
    pub fn load_document(&mut self, outcome: LoadOutcome, path: PathBuf) {
        let active_id = self.active.id;
        self.undo_stack.retain(|(id, _)| *id != active_id);
        self.redo_stack.retain(|(id, _)| *id != active_id);
        self.active.content = outcome.document;
        self.active_stroke = None;
        self.pending_remove = None;
        if let Some(selection) = default_selection(&self.active.content) {
            self.active.selection = selection;
        }
        self.status = Some(load_status(&path, &outcome.warnings));
        self.active.path = Some(path);
        self.active.dirty = false;
    }

    /// Opens a freshly loaded document as a **new** active document, pushing the
    /// previous active into the background (spec/11 §11.1). Unlike Revert, this
    /// discards nothing — the workspace now holds both. In-progress editor interaction
    /// is dropped (it belonged to the previously-active document).
    pub fn open_document(&mut self, outcome: LoadOutcome, path: PathBuf) {
        let selection = default_selection(&outcome.document).unwrap_or(self.active.selection);
        let id = DocumentId::new(self.ids.as_mut());
        let mut document = OpenDocument::new(id, outcome.document, selection);
        document.path = Some(path.clone());
        self.status = Some(load_status(&path, &outcome.warnings));
        let previous = std::mem::replace(&mut self.active, document);
        self.background.insert(0, previous);
        self.active_stroke = None;
        self.pending_remove = None;
    }

    /// Makes the open document `id` active, swapping the current active into the
    /// background (most-recently-active first). A no-op if `id` is already active or
    /// isn't open. Drops in-progress editor interaction, which belonged to the
    /// previously-active document.
    pub fn switch_to(&mut self, id: DocumentId) {
        if self.active.id == id {
            return;
        }
        let Some(index) = self
            .background
            .iter()
            .position(|document| document.id == id)
        else {
            return;
        };
        let target = self.background.remove(index);
        let previous = std::mem::replace(&mut self.active, target);
        self.background.insert(0, previous);
        self.active_stroke = None;
        self.pending_remove = None;
    }

    /// The open documents as `(document, is_active)`, active first, then the background
    /// in most-recently-active order — for the document browser (spec/12 §12.2).
    pub fn open_documents(&self) -> impl Iterator<Item = (&OpenDocument, bool)> {
        std::iter::once((&self.active, true))
            .chain(self.background.iter().map(|document| (document, false)))
    }

    /// How many documents are open (always at least one).
    pub fn open_document_count(&self) -> usize {
        1 + self.background.len()
    }

    /// Whether a Revert is possible: the document is file-bound and has unsaved edits.
    pub fn can_revert(&self) -> bool {
        self.active.path.is_some() && self.active.dirty
    }

    /// Begins a document-replacing action. Returns `true` if it may proceed
    /// immediately (no unsaved edits); returns `false` and arms the confirmation modal
    /// when the document is dirty (spec/12 §12.12).
    pub fn begin_guarded(&mut self, intent: GuardedIntent) -> bool {
        if self.active.dirty {
            self.pending_discard = Some(intent);
            false
        } else {
            true
        }
    }

    /// The action awaiting unsaved-changes confirmation, if the modal is open.
    pub fn pending_discard(&self) -> Option<GuardedIntent> {
        self.pending_discard
    }

    /// Dismisses the unsaved-changes modal, taking the pending intent so the caller can
    /// carry it out (the user chose "Discard"). Returns `None` if nothing was pending.
    pub fn take_pending_discard(&mut self) -> Option<GuardedIntent> {
        self.pending_discard.take()
    }

    /// Cancels the unsaved-changes modal, keeping the current document (spec/12 §12.12).
    pub fn cancel_discard(&mut self) {
        self.pending_discard = None;
    }
}

/// A path's file name for display, falling back to the whole path.
fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// The status line shown after a successful load: the file name, plus a count of any
/// tolerated warnings so the user knows the document loaded but wasn't pristine.
fn load_status(path: &Path, warnings: &[fontspace_model::ValidationWarning]) -> String {
    let name = display_name(path);
    if warnings.is_empty() {
        format!("Opened {name}")
    } else {
        format!("Opened {name} ({} warning(s))", warnings.len())
    }
}

/// A sensible initial selection for a freshly loaded document: the first glyph set's
/// first page, pointed at the first character-set entry (or first stored glyph, or
/// code 0). `None` if the document has no glyph set with a page — the views then show
/// their "nothing selected" guidance until a create-object slice lands.
fn default_selection(document: &FontSpace) -> Option<Selection> {
    let glyph_set = document.glyph_sets.first()?;
    let page = glyph_set.pages.first()?;
    let code = document
        .character_set(glyph_set.character_set_id)
        .and_then(|cs| cs.entries.first().map(|entry| entry.code))
        .or_else(|| page.glyphs.first().map(|glyph| glyph.code))
        .unwrap_or(0);
    Some(Selection {
        glyph_set_id: glyph_set.id,
        page_id: page.id,
        code,
    })
}

/// An 8×8 demo document: a character set with entries `A`–`H`, one glyph set with a
/// single "Regular" page, an `A` drawn for code `0x41`, and a baseline guide. Enough
/// for the editor, page overview, and preview to show real content before file I/O.
fn starter_document(ids: &mut dyn IdGen) -> (FontSpace, Selection) {
    let mut character_set = CharacterSet::new(ids, "ASCII (demo)", "");
    character_set.entries = (0x41..=0x48u32)
        .map(|code| CharacterEntry {
            code,
            label: char::from_u32(code).map(String::from).unwrap_or_default(),
        })
        .collect();

    let size = GlyphSize::new(8, 8);
    let mut glyph_set = GlyphSet::new(ids, "Terminal 8x8", "", size, character_set.id);
    let mut page = GlyphPage::new(ids, "Regular", "");

    // A capital 'A'.
    let mut a = Bitmap::new_blank(size);
    #[rustfmt::skip]
    let pixels = [
        (2, 0), (3, 0), (4, 0), (5, 0),
        (1, 1), (6, 1),
        (1, 2), (6, 2),
        (1, 3), (2, 3), (3, 3), (4, 3), (5, 3), (6, 3),
        (1, 4), (6, 4),
        (1, 5), (6, 5),
        (1, 6), (6, 6),
    ];
    for (x, y) in pixels {
        // Coordinates are in-bounds for an 8×8 glyph, so set cannot fail here.
        a.set(x, y, true).expect("demo pixel within 8×8 bounds");
    }
    page.glyphs.push(Glyph {
        code: 0x41,
        bitmap: a,
    });

    // A baseline guide on the boundary below row 6 (spec/12 §12.6).
    page.guides.push(Guide {
        id: fontspace_model::GuideId::new(ids),
        name: "baseline".into(),
        axis: GuideAxis::Horizontal,
        position: 7,
        visible: true,
        locked: false,
    });

    let selection = Selection {
        glyph_set_id: glyph_set.id,
        page_id: page.id,
        code: 0x41,
    };
    glyph_set.pages.push(page);

    let mut document = FontSpace::new(ids, "Untitled", "");
    document.character_sets.push(character_set);
    document.glyph_sets.push(glyph_set);
    (document, selection)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fontspace_model::SequentialIdGen;

    #[test]
    fn starter_document_selects_a_drawn_glyph() {
        let state = AppState::with_ids(Box::new(SequentialIdGen::new()));
        // The selection resolves and the selected glyph is the drawn 'A'.
        let (glyph_set, _page) = state.selected_context().expect("selection resolves");
        assert_eq!(glyph_set.glyph_size, GlyphSize::new(8, 8));
        assert_eq!(state.selection().code, 0x41);
        let bitmap = state.selected_bitmap().expect("A is drawn");
        assert!(!bitmap.is_blank());
        assert_eq!(state.selected_label(), Some("A"));
    }

    #[test]
    fn select_page_keeps_the_code_point_when_the_target_set_has_it() {
        let mut state = AppState::with_ids(Box::new(SequentialIdGen::new()));
        let sel = state.selection();
        // Move to a different, in-charset code (0x43 = 'C'), then reselect the page.
        state.select_code(0x43);
        state.select_page(sel.glyph_set_id, sel.page_id);
        // The code point is preserved across the (re)selection.
        assert_eq!(state.selection().code, 0x43);
    }

    #[test]
    fn select_page_falls_back_when_the_code_point_is_absent() {
        let mut state = AppState::with_ids(Box::new(SequentialIdGen::new()));
        let sel = state.selection();
        // 0x99 has no entry in the starter charset (A–H); selecting the page must land
        // on a valid code — the first entry, 0x41.
        state.select_code(0x99);
        state.select_page(sel.glyph_set_id, sel.page_id);
        assert_eq!(state.selection().code, 0x41);
    }

    #[test]
    fn starter_document_has_a_visible_baseline_guide() {
        let state = AppState::with_ids(Box::new(SequentialIdGen::new()));
        let (_, page) = state.selected_context().unwrap();
        assert_eq!(page.guides.len(), 1);
        assert!(page.guides[0].visible);
        assert_eq!(page.guides[0].axis, GuideAxis::Horizontal);
    }

    fn editable_state() -> AppState {
        AppState::with_ids(Box::new(SequentialIdGen::new()))
    }

    #[test]
    fn stroke_on_off_pixel_paints_on_and_is_one_undo_entry() {
        let mut state = editable_state();
        // (0, 0) is off in the drawn 'A'; a stroke there paints on.
        assert!(!state.selected_pixel(0, 0));
        state.begin_stroke((0, 0));
        state.extend_stroke((0, 2)); // several cells in one drag
        state.commit_stroke();

        assert!(state.selected_pixel(0, 0));
        assert!(state.selected_pixel(0, 1));
        assert!(state.selected_pixel(0, 2));
        assert!(state.can_undo());
        assert!(!state.can_redo());
        // A whole drag is exactly one undo entry (spec/07 §7.7).
        assert_eq!(state.undo_stack.len(), 1);
    }

    #[test]
    fn stroke_starting_on_an_on_pixel_erases() {
        let mut state = editable_state();
        // (2, 0) is on in the drawn 'A'; a stroke there erases.
        assert!(state.selected_pixel(2, 0));
        state.begin_stroke((2, 0));
        state.commit_stroke();
        assert!(!state.selected_pixel(2, 0));
    }

    #[test]
    fn undo_then_redo_round_trips_a_stroke() {
        let mut state = editable_state();
        state.begin_stroke((0, 0));
        state.extend_stroke((0, 2));
        state.commit_stroke();

        state.undo();
        assert!(!state.selected_pixel(0, 0));
        assert!(!state.selected_pixel(0, 2));
        assert!(!state.can_undo());
        assert!(state.can_redo());

        state.redo();
        assert!(state.selected_pixel(0, 0));
        assert!(state.selected_pixel(0, 2));
        assert!(state.can_undo());
        assert!(!state.can_redo());
    }

    #[test]
    fn remove_impact_preview_counts_cascade_without_mutating() {
        let state = editable_state();
        // The starter doc draws 'A' (0x41); removing it cascades exactly that glyph.
        assert_eq!(state.preview_remove_cascade(0x41), 1);
        // 0x42 has an entry but no glyph → no cascade.
        assert_eq!(state.preview_remove_cascade(0x42), 0);
        // Preview did not touch the document.
        assert!(state.selected_character_set().unwrap().contains_code(0x41));
        assert!(state.selected_pixel(2, 0));
    }

    #[test]
    fn confirm_remove_deletes_entry_and_cascades_as_one_undo_entry() {
        let mut state = editable_state();
        state.request_remove(0x41);
        assert_eq!(state.pending_remove(), Some(0x41));
        state.confirm_remove();
        assert_eq!(state.pending_remove(), None);
        // Entry gone and its glyph cascaded, in one undoable step.
        assert!(!state.selected_character_set().unwrap().contains_code(0x41));
        assert_eq!(state.undo_stack.len(), 1);
        // Undo restores both the entry and the glyph.
        state.undo();
        assert!(state.selected_character_set().unwrap().contains_code(0x41));
        assert!(state.selected_pixel(2, 0));
    }

    #[test]
    fn removing_an_entry_with_no_glyphs_is_still_one_undo_entry() {
        let mut state = editable_state();
        // 0x42 has an entry but no stored glyph → no cascade, yet the entry removal
        // itself is a non-empty, undoable change.
        assert_eq!(state.preview_remove_cascade(0x42), 0);
        state.request_remove(0x42);
        state.confirm_remove();
        assert!(!state.selected_character_set().unwrap().contains_code(0x42));
        assert_eq!(state.undo_stack.len(), 1);
        state.undo();
        assert!(state.selected_character_set().unwrap().contains_code(0x42));
    }

    fn baseline_guide_id(state: &AppState) -> GuideId {
        state.selected_context().unwrap().1.guides[0].id
    }

    #[test]
    fn add_and_remove_guide_are_each_one_undo_entry() {
        let mut state = editable_state();
        // The starter page has one baseline guide.
        assert_eq!(state.selected_context().unwrap().1.guides.len(), 1);
        state.add_guide(GuideAxis::Vertical);
        assert_eq!(state.selected_context().unwrap().1.guides.len(), 2);
        assert_eq!(state.undo_stack.len(), 1);
        state.undo();
        assert_eq!(state.selected_context().unwrap().1.guides.len(), 1);

        let baseline = baseline_guide_id(&state);
        state.remove_guide(baseline);
        assert!(state.selected_context().unwrap().1.guides.is_empty());
        state.undo();
        assert_eq!(state.selected_context().unwrap().1.guides.len(), 1);
    }

    #[test]
    fn toggle_guide_visibility_is_undoable() {
        let mut state = editable_state();
        let baseline = baseline_guide_id(&state);
        assert!(state.selected_context().unwrap().1.guides[0].visible);
        state.set_guide_visible(baseline, false);
        assert!(!state.selected_context().unwrap().1.guides[0].visible);
        state.undo();
        assert!(state.selected_context().unwrap().1.guides[0].visible);
    }

    #[test]
    fn cancel_remove_clears_the_request_without_deleting() {
        let mut state = editable_state();
        state.request_remove(0x41);
        state.cancel_remove();
        assert_eq!(state.pending_remove(), None);
        assert!(state.selected_character_set().unwrap().contains_code(0x41));
        assert!(!state.can_undo());
    }

    #[test]
    fn committing_a_new_stroke_clears_the_redo_stack() {
        let mut state = editable_state();
        state.begin_stroke((0, 0));
        state.commit_stroke();
        state.undo();
        assert!(state.can_redo());
        // A fresh edit discards the redo history.
        state.begin_stroke((7, 7));
        state.commit_stroke();
        assert!(!state.can_redo());
    }

    #[test]
    fn a_fresh_document_is_clean_and_untitled() {
        let state = editable_state();
        assert!(!state.is_dirty());
        assert_eq!(state.path(), None);
        assert_eq!(state.document_name(), "Untitled");
        assert!(!state.can_revert());
    }

    #[test]
    fn editing_marks_dirty_and_saving_marks_clean() {
        let mut state = editable_state();
        state.begin_stroke((0, 0));
        state.commit_stroke();
        assert!(state.is_dirty());

        let path = PathBuf::from("/tmp/demo.fontspace.json");
        state.mark_saved(path.clone());
        assert!(!state.is_dirty());
        assert_eq!(state.path(), Some(path.as_path()));
        assert_eq!(state.document_name(), "demo.fontspace.json");
    }

    #[test]
    fn undo_and_redo_keep_the_document_dirty() {
        // Dirty is conservative: undoing back toward the saved state still reads dirty
        // (safe direction — an unneeded confirm, never silent loss).
        let mut state = editable_state();
        state.begin_stroke((0, 0));
        state.commit_stroke();
        state.mark_saved(PathBuf::from("/tmp/demo.fontspace.json"));
        assert!(!state.is_dirty());
        state.undo();
        assert!(state.is_dirty());
        state.redo();
        assert!(state.is_dirty());
    }

    #[test]
    fn guarded_action_proceeds_when_clean_and_arms_a_modal_when_dirty() {
        let mut state = editable_state();
        // Clean: proceeds immediately, nothing pending.
        assert!(state.begin_guarded(GuardedIntent::Revert));
        assert_eq!(state.pending_discard(), None);

        state.begin_stroke((0, 0));
        state.commit_stroke();
        // Dirty: does not proceed; arms the confirmation.
        assert!(!state.begin_guarded(GuardedIntent::Revert));
        assert_eq!(state.pending_discard(), Some(GuardedIntent::Revert));

        // Cancel keeps the document; take (Discard) hands the intent back once.
        state.cancel_discard();
        assert_eq!(state.pending_discard(), None);
        state.begin_guarded(GuardedIntent::Revert);
        assert_eq!(state.take_pending_discard(), Some(GuardedIntent::Revert));
        assert_eq!(state.take_pending_discard(), None);
    }

    #[test]
    fn can_revert_only_when_file_bound_and_dirty() {
        let mut state = editable_state();
        state.begin_stroke((0, 0));
        state.commit_stroke();
        // Dirty but never saved → nothing to revert to.
        assert!(!state.can_revert());
        state.mark_saved(PathBuf::from("/tmp/demo.fontspace.json"));
        // Saved (clean) → nothing to revert.
        assert!(!state.can_revert());
        state.begin_stroke((7, 7));
        state.commit_stroke();
        // File-bound and dirty → revert is available.
        assert!(state.can_revert());
    }

    #[test]
    fn loading_a_document_resets_history_selection_and_dirt() {
        let mut state = editable_state();
        // Dirty it and stack up history + a pending remove.
        state.begin_stroke((0, 0));
        state.commit_stroke();
        state.request_remove(0x41);
        assert!(state.is_dirty() && state.can_undo());

        // Load a distinct clean document (a fresh starter) bound to a path.
        let fresh = AppState::with_ids(Box::new(SequentialIdGen::new()));
        let outcome = LoadOutcome {
            document: fresh.document().clone(),
            warnings: Vec::new(),
        };
        let path = PathBuf::from("/tmp/opened.fontspace.json");
        state.load_document(outcome, path.clone());

        assert!(!state.is_dirty());
        assert!(!state.can_undo() && !state.can_redo());
        assert_eq!(state.pending_remove(), None);
        assert_eq!(state.path(), Some(path.as_path()));
        // Selection resolves against the loaded document and lands on 'A'.
        assert!(state.selected_context().is_some());
        assert_eq!(state.selection().code, 0x41);
        assert_eq!(state.status(), Some("Opened opened.fontspace.json"));
    }

    /// A fresh starter document wrapped as a `LoadOutcome`, for the multi-document
    /// tests (its own object ids don't matter — `open_document` mints a new id).
    fn loaded_starter() -> LoadOutcome {
        let fresh = AppState::with_ids(Box::new(SequentialIdGen::new()));
        LoadOutcome {
            document: fresh.document().clone(),
            warnings: Vec::new(),
        }
    }

    #[test]
    fn opening_a_document_adds_it_and_keeps_the_previous_in_the_background() {
        let mut state = editable_state();
        // Edit the first document so it has undo history.
        state.begin_stroke((0, 0));
        state.commit_stroke();
        assert_eq!(state.open_document_count(), 1);
        assert!(state.can_undo());

        state.open_document(loaded_starter(), PathBuf::from("/tmp/b.fontspace.json"));

        // Two documents now open; the new one is active and file-bound, the previous
        // moved to the background. The undo history is kept (workspace-level).
        assert_eq!(state.open_document_count(), 2);
        assert_eq!(state.document_name(), "b.fontspace.json");
        assert!(state.can_undo());
        let (active, is_active) = state.open_documents().next().unwrap();
        assert!(is_active);
        assert_eq!(active.display_name(), "b.fontspace.json");
    }

    #[test]
    fn switching_documents_makes_the_target_active() {
        let mut state = editable_state();
        let first_id = state.open_documents().next().unwrap().0.id;
        state.open_document(loaded_starter(), PathBuf::from("/tmp/b.fontspace.json"));
        // Now b is active, the first document is in the background.
        assert_ne!(state.open_documents().next().unwrap().0.id, first_id);

        state.switch_to(first_id);
        assert_eq!(state.open_documents().next().unwrap().0.id, first_id);
        // Switching to the already-active document is a no-op.
        state.switch_to(first_id);
        assert_eq!(state.open_document_count(), 2);
    }

    #[test]
    fn undo_targets_the_originating_document_after_switching() {
        // The load-bearing multi-document invariant: an edit's undo applies to the
        // document it came from, even once another document is active (spec/11 §11.6).
        let mut state = editable_state();
        let first_id = state.open_documents().next().unwrap().0.id;
        // Erase 'A''s on-pixel (2,0) in the first document.
        assert!(state.selected_pixel(2, 0));
        state.begin_stroke((2, 0));
        state.commit_stroke();
        assert!(!state.selected_pixel(2, 0));

        // Open a second document (fresh 'A' drawn); it becomes active.
        state.open_document(loaded_starter(), PathBuf::from("/tmp/b.fontspace.json"));
        assert!(state.selected_pixel(2, 0)); // b's 'A' is intact

        // Undo — must revert the *first* document, not the active second one.
        state.undo();
        assert!(state.selected_pixel(2, 0)); // b untouched

        // Back on the first document, its erase has been undone (pixel restored).
        state.switch_to(first_id);
        assert!(state.selected_pixel(2, 0));
        assert!(state.can_redo());
    }
}
