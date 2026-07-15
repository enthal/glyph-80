//! Application workspace state: the open FontSpace document(s) the GUI edits, the
//! active document's selection, and editor view options.
//!
//! The workspace holds its documents as [`OpenDocument`]s ([`crate::workspace`]); this
//! slice keeps exactly one, the **active** document, and exposes it through an
//! active-document facade ([`AppState::document`], [`AppState::selection`], …). Opening
//! several at once and switching between them arrive in a later slice (spec/11). The
//! GUI owns this state but **not** font semantics — mutations go through
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

/// A file action that would replace the current document, held pending confirmation
/// while there are unsaved changes (spec/12 §12.12). The guard prevents a click from
/// silently discarding edits; the user confirms or cancels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuardedIntent {
    /// Choose a file and open it in place of the current document.
    Open,
    /// Reload the current document from its file on disk, discarding edits.
    Revert,
}

/// The editable state of the workspace: the active open document plus view options.
///
/// The active document's content, path, dirty flag, and selection live on its
/// [`OpenDocument`] (`active`) and are reached through the facade accessors below, so
/// generalizing to several open documents later touches only this struct — not the
/// views. Path binding, the dirty flag (spec/11 §11.2), and the unsaved-changes guard
/// are conservative: dirty may read true after undoing back to the saved state, which
/// only ever asks for an unneeded confirm, never risks silent data loss.
pub struct AppState {
    /// The active open document — the one the views render and edit. A workspace of
    /// several documents with switching arrives in a later slice (spec/11 §11.1).
    active: OpenDocument,
    /// Injected id source for object-creating operations (spec/03 §Id injection).
    pub ids: Box<dyn IdGen>,
    pub grid: GridLevel,
    /// Editable sample text for the text-preview view (spec/12 §12.10). UI state,
    /// not document data — it is never written to the `.fontspace.json`.
    pub preview_text: String,
    /// A document-replacing action awaiting unsaved-changes confirmation (spec/12
    /// §12.12), or `None` when no modal is open.
    pending_discard: Option<GuardedIntent>,
    /// The most recent save/open result or error, shown in the status strip. It
    /// persists until the next file action replaces it (the dirty marker, not this
    /// line, is the authoritative unsaved-state signal).
    status: Option<String>,
    /// The stroke currently being dragged in the editor, if any (spec/12 §12.4).
    active_stroke: Option<Stroke>,
    /// Workspace-level undo/redo stacks of committed change sets (spec/07 §7.7). A
    /// single-document workspace for now; multi-document lands in Milestone 3.
    undo_stack: Vec<ChangeSet>,
    redo_stack: Vec<ChangeSet>,
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
            ids,
            grid: GridLevel::Subtle,
            preview_text: "AAA HAH".to_string(),
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

    /// Records a committed change on the undo stack (clearing redo) and marks the
    /// document dirty, skipping an empty (no-op) change set. The single place edits
    /// enter the history — so it is also the single place the dirty flag is raised.
    fn record(&mut self, change_set: ChangeSet) {
        if !change_set.is_empty() {
            self.undo_stack.push(change_set);
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

    /// Undoes the most recent committed change (spec/07 §7.7).
    pub fn undo(&mut self) {
        if let Some(change_set) = self.undo_stack.pop() {
            // The change set came from this document, so its inverse applies cleanly.
            let _ = undo(&mut self.active.content, &change_set);
            self.redo_stack.push(change_set);
            self.active.dirty = true;
        }
    }

    /// Redoes the most recently undone change (spec/07 §7.7).
    pub fn redo(&mut self) {
        if let Some(change_set) = self.redo_stack.pop() {
            let _ = apply_change_set(&mut self.active.content, &change_set);
            self.undo_stack.push(change_set);
            self.active.dirty = true;
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

    /// Replaces the document with a freshly loaded one bound to `path` (Open/Revert):
    /// resets selection, clears history and any in-progress interaction, and marks the
    /// document clean. Load warnings (e.g. dangling glyphs) go to the status strip.
    pub fn load_document(&mut self, outcome: LoadOutcome, path: PathBuf) {
        self.active.content = outcome.document;
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.active_stroke = None;
        self.pending_remove = None;
        if let Some(selection) = default_selection(&self.active.content) {
            self.active.selection = selection;
        }
        self.status = Some(load_status(&path, &outcome.warnings));
        self.active.path = Some(path);
        self.active.dirty = false;
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
        assert!(state.begin_guarded(GuardedIntent::Open));
        assert_eq!(state.pending_discard(), None);

        state.begin_stroke((0, 0));
        state.commit_stroke();
        // Dirty: does not proceed; arms the confirmation.
        assert!(!state.begin_guarded(GuardedIntent::Open));
        assert_eq!(state.pending_discard(), Some(GuardedIntent::Open));

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
}
