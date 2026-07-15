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

use fontspace_json::{LoadOutcome, load_fragment, save_fragment};
use fontspace_model::{
    Bitmap, CharacterEntry, CharacterSet, CharacterSetId, FontSpace, FontSpaceFragment,
    FragmentGlyph, Glyph, GlyphFragment, GlyphPage, GlyphSet, GlyphSetId, GlyphSize, Guide,
    GuideAxis, GuideId, IdGen, OverflowPolicy, PageId, RandomIdGen,
};

use fontspace_ops::{
    AddGuide, ChangeSet, ClearGlyphs, FontSpaceWarning, GlyphMapping, GlyphRef, GlyphSelector,
    GlyphSizeConversion, InvertGlyphs, MoveGuide, PageSelector, PasteGlyphs, RemoveCharacterEntry,
    RemoveGuide, RenameGuide, SetGuideVisible, SetPixels, ShiftGlyphs, add_guide, apply_change_set,
    clear_glyphs, invert_glyphs, move_guide, paste_glyphs, remove_character_entry, remove_guide,
    rename_guide, set_guide_visible, set_pixels, shift_glyphs, undo,
};

use crate::editor::geometry::GridLevel;
use crate::editor::region::{
    FlipDir, PixelRect, flip_edits, region_extract, rotate_edits, stamp_edits,
};
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
/// document — so it is unguarded.)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuardedIntent {
    /// Reload the active document from its file on disk, discarding its edits.
    Revert,
    /// Close the active document, discarding its edits.
    Close,
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
    /// The rectangular pixel-region marquee in the editor (spec/12 §12.4), or `None`.
    /// UI state, tied to the current glyph's grid; cleared when the selection navigates
    /// to another glyph. Set by Shift+drag.
    pixel_selection: Option<PixelRect>,
    /// The fixed corner cell of an in-progress selection drag, or `None` between drags.
    selection_anchor: Option<(u16, u16)>,
    /// The copied pixel region (spec/12 §12.4), a patch stamped by paste. Survives
    /// navigation (it is a clipboard), unlike the marquee. UI state, never persisted.
    region_clipboard: Option<Bitmap>,
    /// The drag-selected run of codes in the page overview (spec/12 §12.8), in display
    /// order; empty when nothing is range-selected. UI state, cleared when the selection
    /// navigates to another glyph or the active document changes. Feeds multi-glyph copy.
    page_glyph_selection: Vec<u32>,
    /// The anchor code of an in-progress page-overview drag-select, or `None` between
    /// drags (the counterpart to `selection_anchor` for the pixel marquee).
    page_selection_anchor: Option<u32>,
    /// Whether the glyph-shift control wraps pixels around the opposite edge
    /// (`OverflowPolicy::Wrap`) rather than discarding them (spec/12 §12.3). UI state;
    /// default off, matching the CLI `shift` default. Wrap rotates rows/columns.
    pub shift_wrap: bool,
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
            pixel_selection: None,
            selection_anchor: None,
            region_clipboard: None,
            page_glyph_selection: Vec::new(),
            page_selection_anchor: None,
            shift_wrap: false,
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
        self.clear_selection(); // the marquee is tied to the glyph it was drawn on
        self.clear_page_selection(); // a single click supersedes a range selection
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
        self.clear_selection(); // the marquee is tied to the glyph it was drawn on
        self.clear_page_selection(); // the range was tied to the page it was drawn on
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

    // --- Rectangular pixel-region selection (spec/12 §12.4). Shift+drag defines a
    // marquee; region transforms apply as one `SetPixels` — one undo entry. ---

    /// The current pixel-region marquee, for rendering and region ops.
    pub fn pixel_selection(&self) -> Option<PixelRect> {
        self.pixel_selection
    }

    /// Whether a selection drag is in progress (routes further drag to the marquee).
    pub fn is_selecting(&self) -> bool {
        self.selection_anchor.is_some()
    }

    /// Begins a selection drag anchored at `cell` (a 1-cell marquee to start).
    pub fn begin_selection(&mut self, cell: (u16, u16)) {
        self.selection_anchor = Some(cell);
        self.pixel_selection = Some(PixelRect::from_corners(cell, cell));
    }

    /// Extends the in-progress selection to `cell`.
    pub fn extend_selection(&mut self, cell: (u16, u16)) {
        if let Some(anchor) = self.selection_anchor {
            self.pixel_selection = Some(PixelRect::from_corners(anchor, cell));
        }
    }

    /// Ends the selection drag; the marquee persists until cleared or re-selected.
    pub fn end_selection(&mut self) {
        self.selection_anchor = None;
    }

    /// Clears the marquee (the Clear button, or navigating to another glyph/document).
    pub fn clear_selection(&mut self) {
        self.pixel_selection = None;
        self.selection_anchor = None;
    }

    // --- Page-overview range selection (spec/12 §12.8). Drag-select a run of glyph
    // codes to copy them together as one multi-glyph fragment. ---

    /// The drag-selected run of codes in the page overview, in display order (empty when
    /// nothing is range-selected).
    pub fn page_glyph_selection(&self) -> &[u32] {
        &self.page_glyph_selection
    }

    /// Whether a page-overview drag-select is in progress (an anchor is held).
    pub fn is_page_selecting(&self) -> bool {
        self.page_selection_anchor.is_some()
    }

    /// The anchor code of the in-progress drag-select, for the view to resolve the range.
    pub fn page_selection_anchor(&self) -> Option<u32> {
        self.page_selection_anchor
    }

    /// Begins a page-overview drag-select anchored at `code` (a one-code run to start).
    pub fn begin_page_selection(&mut self, code: u32) {
        self.page_selection_anchor = Some(code);
        self.page_glyph_selection = vec![code];
    }

    /// Sets the drag-selected run to `codes` (the view resolves the inclusive range from
    /// the anchor against its ordered entries — spec/12 §12.8).
    pub fn set_page_selection_range(&mut self, codes: Vec<u32>) {
        self.page_glyph_selection = codes;
    }

    /// Ends the drag; the range persists until cleared, re-selected, or navigated away.
    pub fn end_page_selection(&mut self) {
        self.page_selection_anchor = None;
    }

    /// Clears the page-overview range selection.
    pub fn clear_page_selection(&mut self) {
        self.page_glyph_selection.clear();
        self.page_selection_anchor = None;
    }

    /// The selected run restricted to codes that have a character-set entry — the batch
    /// transforms below are charset-resolved, so a **dangling** code in the run (stored
    /// but not in the set) is left out rather than failing the whole op. Preserves the
    /// run's display order.
    fn charset_codes_in_page_selection(&self) -> Vec<u32> {
        let character_set = self.selected_character_set();
        self.page_glyph_selection
            .iter()
            .copied()
            .filter(|code| character_set.is_some_and(|cs| cs.contains_code(*code)))
            .collect()
    }

    /// Blanks every stored glyph in the page-overview run as one undo entry (spec/12
    /// §12.8), reusing the `ClearGlyphs` batch op. Absent glyphs are left untouched
    /// (not materialized). Returns how many in-charset codes the run targeted (`0` when
    /// the run is empty or all dangling — a no-op), for the caller's status line.
    pub fn blank_page_selection(&mut self) -> usize {
        let codes = self.charset_codes_in_page_selection();
        if codes.is_empty() {
            return 0;
        }
        let count = codes.len();
        let request = ClearGlyphs {
            glyph_set_id: self.active.selection.glyph_set_id,
            pages: PageSelector::Id(self.active.selection.page_id),
            glyphs: GlyphSelector::Codes(codes),
        };
        if let Ok(change_set) = clear_glyphs(&mut self.active.content, &request) {
            self.record(change_set);
        }
        count
    }

    /// Inverts (toggles every pixel of) each stored glyph in the page-overview run as
    /// one undo entry (spec/12 §12.8), reusing the `InvertGlyphs` batch op. Absent
    /// glyphs are left untouched — invert does not fill blank codes with all-on glyphs
    /// (spec/07 §7.5). Returns how many in-charset codes the run targeted (`0` on a
    /// no-op), for the caller's status line.
    pub fn invert_page_selection(&mut self) -> usize {
        let codes = self.charset_codes_in_page_selection();
        if codes.is_empty() {
            return 0;
        }
        let count = codes.len();
        let request = InvertGlyphs {
            glyph_set_id: self.active.selection.glyph_set_id,
            pages: PageSelector::Id(self.active.selection.page_id),
            glyphs: GlyphSelector::Codes(codes),
        };
        if let Ok(change_set) = invert_glyphs(&mut self.active.content, &request) {
            self.record(change_set);
        }
        count
    }

    /// Shifts every stored glyph in the page-overview run by `(dx, dy)` as one undo
    /// entry (spec/12 §12.8), reusing the `ShiftGlyphs` batch op. The overflow policy
    /// follows [`shift_wrap`](Self::shift_wrap) (shared with the editor's shift control):
    /// wrap rotates rows/columns around the far edge, discard drops what falls off.
    /// Absent glyphs are left untouched. Returns how many in-charset codes the run
    /// targeted (`0` on a no-op), for the caller's status line.
    pub fn shift_page_selection(&mut self, dx: i16, dy: i16) -> usize {
        let codes = self.charset_codes_in_page_selection();
        if codes.is_empty() {
            return 0;
        }
        let count = codes.len();
        let overflow = if self.shift_wrap {
            OverflowPolicy::Wrap
        } else {
            OverflowPolicy::Discard
        };
        let request = ShiftGlyphs {
            glyph_set_id: self.active.selection.glyph_set_id,
            pages: PageSelector::Id(self.active.selection.page_id),
            glyphs: GlyphSelector::Codes(codes),
            dx,
            dy,
            overflow,
        };
        if let Ok(change_set) = shift_glyphs(&mut self.active.content, &request) {
            self.record(change_set);
        }
        count
    }

    /// Mirrors the selected region in place (`dir`) as one undo entry — the "reverse"
    /// (spec/12 §12.4). A no-op when there is no selection or the region is symmetric.
    pub fn flip_selection(&mut self, dir: FlipDir) {
        let Some(rect) = self.pixel_selection else {
            return;
        };
        let Some((glyph_set, _)) = self.selected_context() else {
            return;
        };
        let size = glyph_set.glyph_size;
        let bitmap = self
            .selected_bitmap()
            .cloned()
            .unwrap_or_else(|| Bitmap::new_blank(size));
        let request = SetPixels {
            target: GlyphRef {
                glyph_set_id: self.active.selection.glyph_set_id,
                page_id: self.active.selection.page_id,
                code: self.active.selection.code,
            },
            edits: flip_edits(&bitmap, rect, dir),
        };
        if let Ok(change_set) = set_pixels(&mut self.active.content, &request) {
            self.record(change_set);
        }
    }

    /// Rotates the selected region 90° clockwise in place as one undo entry (spec/12
    /// §12.4). A square region rotates within its bounds; a non-square one rotates into
    /// its transposed footprint (anchored at the top-left, clipped at the glyph edge),
    /// and the marquee follows it there. A no-op when there is no selection.
    pub fn rotate_selection(&mut self) {
        let Some(rect) = self.pixel_selection else {
            return;
        };
        let Some((glyph_set, _)) = self.selected_context() else {
            return;
        };
        let size = glyph_set.glyph_size;
        let bitmap = self
            .selected_bitmap()
            .cloned()
            .unwrap_or_else(|| Bitmap::new_blank(size));
        let request = SetPixels {
            target: GlyphRef {
                glyph_set_id: self.active.selection.glyph_set_id,
                page_id: self.active.selection.page_id,
                code: self.active.selection.code,
            },
            edits: rotate_edits(&bitmap, rect, size),
        };
        if let Ok(change_set) = set_pixels(&mut self.active.content, &request) {
            self.record(change_set);
            // The marquee follows the content into the transposed footprint (dims
            // swapped), clamped to the glyph so it never leaves the matrix.
            self.pixel_selection = Some(PixelRect {
                x0: rect.x0,
                y0: rect.y0,
                x1: (rect.x0 + rect.height() - 1).min(size.width.saturating_sub(1)),
                y1: (rect.y0 + rect.width() - 1).min(size.height.saturating_sub(1)),
            });
        }
    }

    /// Whether a region has been copied and can be pasted.
    pub fn has_region_clipboard(&self) -> bool {
        self.region_clipboard.is_some()
    }

    /// Copies the selected region's pixels to the region clipboard (spec/12 §12.4). The
    /// clipboard survives navigation, so you can copy on one glyph and paste on another.
    pub fn copy_selection(&mut self) {
        let Some(rect) = self.pixel_selection else {
            return;
        };
        let Some((glyph_set, _)) = self.selected_context() else {
            return;
        };
        let size = glyph_set.glyph_size;
        let bitmap = self
            .selected_bitmap()
            .cloned()
            .unwrap_or_else(|| Bitmap::new_blank(size));
        self.region_clipboard = Some(region_extract(&bitmap, rect));
    }

    /// Stamps the region clipboard onto the current glyph with its top-left at the
    /// marquee's top-left corner, replacing those cells — one undo entry (spec/12
    /// §12.4). A no-op with no clipboard or no selection anchor.
    pub fn paste_region(&mut self) {
        let Some(patch) = self.region_clipboard.clone() else {
            return;
        };
        let Some(rect) = self.pixel_selection else {
            return;
        };
        let Some((glyph_set, _)) = self.selected_context() else {
            return;
        };
        let size = glyph_set.glyph_size;
        let request = SetPixels {
            target: GlyphRef {
                glyph_set_id: self.active.selection.glyph_set_id,
                page_id: self.active.selection.page_id,
                code: self.active.selection.code,
            },
            edits: stamp_edits(&patch, (rect.x0, rect.y0), size),
        };
        if let Ok(change_set) = set_pixels(&mut self.active.content, &request) {
            self.record(change_set);
        }
    }

    /// Shifts the whole current glyph by `(dx, dy)` as one undo entry (spec/12 §12.3),
    /// reusing the `ShiftGlyphs` domain op that backs the CLI `shift`. The overflow
    /// policy follows [`shift_wrap`](Self::shift_wrap): wrap rotates rows/columns around
    /// the opposite edge, discard drops what falls off. A no-op when the current glyph is
    /// absent or the shift changes nothing (a blank glyph, or `(0, 0)`).
    pub fn shift_glyph(&mut self, dx: i16, dy: i16) {
        let overflow = if self.shift_wrap {
            OverflowPolicy::Wrap
        } else {
            OverflowPolicy::Discard
        };
        let request = ShiftGlyphs {
            glyph_set_id: self.active.selection.glyph_set_id,
            pages: PageSelector::Id(self.active.selection.page_id),
            glyphs: GlyphSelector::Code(self.active.selection.code),
            dx,
            dy,
            overflow,
        };
        if let Ok(change_set) = shift_glyphs(&mut self.active.content, &request) {
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

    /// Renames a guide as one undo entry (a no-op if the name is unchanged).
    pub fn rename_guide(&mut self, guide_id: GuideId, name: String) {
        let request = RenameGuide {
            glyph_set_id: self.active.selection.glyph_set_id,
            page_id: self.active.selection.page_id,
            guide_id,
            name,
        };
        if let Ok(change_set) = rename_guide(&mut self.active.content, &request) {
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

    /// Drops every undo/redo entry tagged with `id` from both stacks. Used when a
    /// document leaves the workspace-level history — on Revert (reload in place) and
    /// on Close (spec/11 §11.6) — so no orphaned entry can target a stale document.
    fn drop_history_of(&mut self, id: DocumentId) {
        self.undo_stack.retain(|(entry_id, _)| *entry_id != id);
        self.redo_stack.retain(|(entry_id, _)| *entry_id != id);
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

    /// Serializes the selected glyph as a single-glyph fragment for the clipboard
    /// (spec/08 §8.2). Returns the canonical fragment JSON, or `None` if the selection
    /// no longer resolves. An undrawn cell copies as a blank glyph of the set's
    /// geometry — pasting it elsewhere clears that target.
    pub fn copy_selected_glyph(&self) -> Option<String> {
        self.fragment_for_codes(&[self.active.selection.code])
    }

    /// Serializes the page-overview range selection as a multi-glyph fragment for the
    /// clipboard (spec/12 §12.8), in display order. Undrawn codes in the run copy as
    /// blank glyphs so the block's layout is preserved on paste. `None` when nothing is
    /// range-selected or the selection no longer resolves.
    pub fn copy_page_selection(&self) -> Option<String> {
        if self.page_glyph_selection.is_empty() {
            return None;
        }
        self.fragment_for_codes(&self.page_glyph_selection)
    }

    /// The fragment JSON the clipboard should carry for `Cmd/Ctrl+C`: the page-overview
    /// range when one is drag-selected, otherwise the single selected glyph.
    pub fn copy_glyphs_for_clipboard(&self) -> Option<String> {
        if self.page_glyph_selection.is_empty() {
            self.copy_selected_glyph()
        } else {
            self.copy_page_selection()
        }
    }

    /// Builds a canonical glyph-fragment JSON for `codes` (in the given order) from the
    /// current page (spec/08 §8.2). Each undrawn code copies as a blank glyph of the
    /// set's geometry — pasting it elsewhere clears that target. `None` if the selection
    /// no longer resolves or `codes` is empty.
    fn fragment_for_codes(&self, codes: &[u32]) -> Option<String> {
        if codes.is_empty() {
            return None;
        }
        let (glyph_set, page) = self.selected_context()?;
        let character_set = self.selected_character_set();
        let glyphs = codes
            .iter()
            .map(|&code| {
                let bitmap = page
                    .glyph_of_code(code)
                    .map(|glyph| glyph.bitmap.clone())
                    .unwrap_or_else(|| Bitmap::new_blank(glyph_set.glyph_size));
                let label = character_set
                    .and_then(|cs| cs.entry(code))
                    .map(|entry| entry.label.clone())
                    .unwrap_or_default();
                FragmentGlyph {
                    code,
                    label,
                    bitmap,
                }
            })
            .collect();
        let fragment = FontSpaceFragment::Glyphs(GlyphFragment {
            source_glyph_size: glyph_set.glyph_size,
            glyphs,
        });
        Some(save_fragment(&fragment))
    }

    /// Pastes a clipboard glyph fragment onto the **current** selection as one undo
    /// entry (spec/08 §8.2). The fragment's glyphs map onto the selected code and the
    /// codes numerically after it (`SequentialFromCode`), so a single copied glyph
    /// lands exactly where the editor is pointed. Geometry must match (`RequireExact`).
    /// Returns a human-readable error for the status strip on a non-fragment clipboard,
    /// a geometry mismatch, or a destination code with no entry.
    pub fn paste_glyph_from_clipboard(&mut self, clipboard: &str) -> Result<(), String> {
        let FontSpaceFragment::Glyphs(fragment) = load_fragment(clipboard)
            .map_err(|err| format!("Clipboard is not a glyph fragment: {err}"))?;
        let request = PasteGlyphs {
            fragment,
            target_glyph_set_id: self.active.selection.glyph_set_id,
            target_page_id: self.active.selection.page_id,
            mapping: GlyphMapping::SequentialFromCode(self.active.selection.code),
            size_conversion: GlyphSizeConversion::RequireExact,
        };
        match paste_glyphs(&mut self.active.content, &request) {
            Ok(change_set) => {
                self.record(change_set);
                Ok(())
            }
            Err(err) => Err(format!("Paste failed: {err}")),
        }
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

    /// Sets the status strip to an informational message (e.g. a clipboard action).
    pub fn set_status(&mut self, message: impl Into<String>) {
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
        self.drop_history_of(self.active.id);
        self.active.content = outcome.document;
        self.active_stroke = None;
        self.pixel_selection = None; // the marquee was tied to the pre-revert glyph
        self.selection_anchor = None;
        self.page_glyph_selection.clear();
        self.page_selection_anchor = None;
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
        self.discard_active_interaction();
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
        self.discard_active_interaction();
    }

    /// Clears interaction state tied to the previously-active document when the active
    /// document changes: the in-progress stroke, the pixel-region marquee (tied to the
    /// glyph it was drawn on), the pending remove, and any pending unsaved-changes guard
    /// (which was about the document that just stepped aside).
    fn discard_active_interaction(&mut self) {
        self.active_stroke = None;
        self.pixel_selection = None;
        self.selection_anchor = None;
        self.page_glyph_selection.clear();
        self.page_selection_anchor = None;
        self.pending_remove = None;
        self.pending_discard = None;
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

    /// Whether the active document can be closed: only when another remains open. The
    /// workspace always keeps at least one document; a `New` empty document arrives
    /// with a later slice.
    pub fn can_close(&self) -> bool {
        !self.background.is_empty()
    }

    /// Closes the active document, promoting the most-recently-active background
    /// document in its place and dropping the closed document's undo/redo entries. A
    /// no-op if it is the only open document (spec/12 §12.12).
    pub fn close_active(&mut self) {
        if self.background.is_empty() {
            return;
        }
        let closed_id = self.active.id;
        let closed_name = self.active.display_name();
        self.drop_history_of(closed_id);
        self.active = self.background.remove(0);
        self.status = Some(format!("Closed {closed_name}"));
        self.discard_active_interaction();
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
    fn copy_selected_glyph_serializes_the_selected_cell() {
        let state = editable_state();
        assert_eq!(state.selection().code, 0x41); // the drawn 'A'
        let json = state.copy_selected_glyph().unwrap();
        let FontSpaceFragment::Glyphs(fragment) = load_fragment(&json).unwrap();
        assert_eq!(fragment.glyphs.len(), 1);
        assert_eq!(fragment.glyphs[0].code, 0x41);
        assert_eq!(&fragment.glyphs[0].bitmap, state.selected_bitmap().unwrap());
    }

    #[test]
    fn copy_then_paste_lands_the_glyph_on_the_new_selection() {
        let mut state = editable_state();
        assert!(state.selected_pixel(2, 0)); // 'A' (0x41) has (2,0) on
        let json = state.copy_selected_glyph().unwrap();

        // Point the editor at a currently-blank code and paste there.
        state.select_code(0x42);
        assert!(!state.selected_pixel(2, 0));
        state.paste_glyph_from_clipboard(&json).unwrap();
        assert!(state.selected_pixel(2, 0)); // 0x42 now carries 'A''s pixels
        assert!(state.can_undo());

        // The paste is one undo entry.
        state.undo();
        assert!(!state.selected_pixel(2, 0));
    }

    #[test]
    fn copy_page_selection_serializes_the_selected_run_in_order() {
        let mut state = editable_state();
        // Drag-select the run A..C (as the page overview would).
        state.begin_page_selection(0x41);
        state.set_page_selection_range(vec![0x41, 0x42, 0x43]);
        assert_eq!(state.page_glyph_selection(), &[0x41, 0x42, 0x43]);

        let json = state.copy_page_selection().unwrap();
        let FontSpaceFragment::Glyphs(fragment) = load_fragment(&json).unwrap();
        let codes: Vec<u32> = fragment.glyphs.iter().map(|g| g.code).collect();
        assert_eq!(codes, vec![0x41, 0x42, 0x43]); // display order, blanks included
    }

    #[test]
    fn copy_glyphs_for_clipboard_prefers_a_range_over_the_single_glyph() {
        let mut state = editable_state();
        // No range: falls back to the single selected glyph (one entry).
        let single = state.copy_glyphs_for_clipboard().unwrap();
        let FontSpaceFragment::Glyphs(one) = load_fragment(&single).unwrap();
        assert_eq!(one.glyphs.len(), 1);

        // With a range: copies the whole run.
        state.begin_page_selection(0x41);
        state.set_page_selection_range(vec![0x41, 0x42]);
        let many = state.copy_glyphs_for_clipboard().unwrap();
        let FontSpaceFragment::Glyphs(two) = load_fragment(&many).unwrap();
        assert_eq!(two.glyphs.len(), 2);
    }

    #[test]
    fn multi_glyph_copy_pastes_the_run_sequentially() {
        let mut state = editable_state();
        assert!(state.selected_pixel(2, 0)); // 'A' (0x41) has (2,0) on
        state.begin_page_selection(0x41);
        state.set_page_selection_range(vec![0x41, 0x42]);
        let json = state.copy_page_selection().unwrap();

        // Paste starting at a blank code: the run lands on 0x47, 0x48 sequentially.
        state.select_code(0x47); // also clears the range (single-code navigation)
        assert!(state.page_glyph_selection().is_empty());
        assert!(!state.selected_pixel(2, 0));
        state.paste_glyph_from_clipboard(&json).unwrap();
        assert!(state.selected_pixel(2, 0)); // 0x47 now carries A's pixels
        assert!(state.can_undo());
        state.undo(); // one undo entry
        assert!(!state.selected_pixel(2, 0));
    }

    #[test]
    fn invert_page_selection_toggles_the_run_and_is_one_undo_entry() {
        let mut state = editable_state();
        assert!(state.selected_pixel(2, 0)); // 'A' (0x41) has (2,0) on
        // Select the run A..C and invert it.
        state.begin_page_selection(0x41);
        state.set_page_selection_range(vec![0x41, 0x42, 0x43]);
        state.invert_page_selection();

        // 0x41 is stored ('A'), so (2,0) toggles off; the op is one undo entry.
        assert!(!state.selected_pixel(2, 0));
        assert!(state.can_undo());
        state.undo();
        assert!(state.selected_pixel(2, 0));
    }

    #[test]
    fn shift_page_selection_moves_the_run_and_is_one_undo_entry() {
        let mut state = editable_state();
        assert!(!state.shift_wrap); // discard by default
        let before = state.selected_bitmap().expect("A is drawn").clone();
        assert!(state.selected_pixel(2, 0)); // 'A' (0x41) has a pixel at (2,0)

        state.begin_page_selection(0x41);
        state.set_page_selection_range(vec![0x41, 0x42, 0x43]);
        let count = state.shift_page_selection(1, 0); // shift the run right by one
        assert_eq!(count, 3);

        // 'A' (the drawn glyph in the run) moved: leftmost content shifted right, so
        // the glyph changed; one undo entry restores it exactly.
        let after = state.selected_bitmap().expect("A still drawn");
        assert_ne!(after, &before);
        assert!(state.selected_pixel(3, 0)); // (2,0) carried to (3,0)
        assert!(state.can_undo());
        state.undo();
        assert_eq!(state.selected_bitmap().expect("A restored"), &before);
    }

    #[test]
    fn blank_page_selection_clears_stored_glyphs_in_the_run() {
        let mut state = editable_state();
        assert!(!state.selected_bitmap().unwrap().is_blank()); // 'A' is drawn
        state.begin_page_selection(0x41);
        state.set_page_selection_range(vec![0x41, 0x42]);
        state.blank_page_selection();

        // 'A' (0x41) is now blank; one undo entry restores it.
        assert!(state.selected_bitmap().is_none_or(|b| b.is_blank()));
        assert!(state.can_undo());
        state.undo();
        assert!(!state.selected_bitmap().unwrap().is_blank());
    }

    #[test]
    fn page_glyph_selection_clears_when_navigating_to_a_code() {
        let mut state = editable_state();
        state.begin_page_selection(0x41);
        state.set_page_selection_range(vec![0x41, 0x42, 0x43]);
        assert!(!state.page_glyph_selection().is_empty());

        state.select_code(0x45);
        assert!(state.page_glyph_selection().is_empty());
        assert!(!state.is_page_selecting());
    }

    #[test]
    fn paste_of_non_fragment_text_is_reported_not_panicking() {
        let mut state = editable_state();
        let err = state
            .paste_glyph_from_clipboard("not a fragment")
            .unwrap_err();
        assert!(err.contains("not a glyph fragment"), "{err}");
        assert!(!state.can_undo()); // nothing changed
    }

    #[test]
    fn rename_guide_updates_the_name_and_is_one_undo_entry() {
        let mut state = editable_state();
        // The starter has a "baseline" guide on the selected (Regular) page.
        let guide_id = state.selected_context().unwrap().1.guides[0].id;
        assert_eq!(
            state.selected_context().unwrap().1.guides[0].name,
            "baseline"
        );

        state.rename_guide(guide_id, "cap height".to_string());
        assert_eq!(
            state.selected_context().unwrap().1.guides[0].name,
            "cap height"
        );
        assert!(state.can_undo());

        state.undo();
        assert_eq!(
            state.selected_context().unwrap().1.guides[0].name,
            "baseline"
        );
    }

    #[test]
    fn flip_selection_mirrors_the_region_and_is_one_undo_entry() {
        let mut state = editable_state();
        // The starter's 'A' is left-right symmetric, so paint an asymmetric pixel in
        // the unused column 0 to make the flip observable.
        state.begin_stroke((0, 0));
        state.commit_stroke();
        assert!(state.selected_pixel(0, 0));
        assert!(!state.selected_pixel(7, 0));

        // Select the whole glyph and mirror left↔right.
        state.begin_selection((0, 0));
        state.extend_selection((7, 7));
        state.end_selection();
        state.flip_selection(FlipDir::LeftRight);

        assert!(state.selected_pixel(7, 0)); // (0,0) mirrored to (7,0)
        assert!(!state.selected_pixel(0, 0));
        assert!(state.can_undo());

        // Undo the flip alone (the paint remains): (0,0) is back, (7,0) clear.
        state.undo();
        assert!(state.selected_pixel(0, 0));
        assert!(!state.selected_pixel(7, 0));
    }

    #[test]
    fn copy_then_paste_stamps_the_region_and_is_one_undo_entry() {
        let mut state = editable_state();
        state.begin_stroke((0, 0)); // paint (0,0) on (column 0 is unused by 'A')
        state.commit_stroke();

        // Copy the single on-cell (0,0).
        state.begin_selection((0, 0));
        state.extend_selection((0, 0));
        state.end_selection();
        state.copy_selection();
        assert!(state.has_region_clipboard());

        // Move the marquee to an empty cell (7,7) and paste.
        state.begin_selection((7, 7));
        state.extend_selection((7, 7));
        state.end_selection();
        assert!(!state.selected_pixel(7, 7));
        state.paste_region();
        assert!(state.selected_pixel(7, 7)); // the on-patch stamped at (7,7)
        assert!(state.can_undo());

        state.undo(); // the paste was one undoable entry
        assert!(!state.selected_pixel(7, 7));
    }

    #[test]
    fn region_clipboard_survives_navigation() {
        let mut state = editable_state();
        state.begin_stroke((0, 0));
        state.commit_stroke();
        state.begin_selection((0, 0));
        state.extend_selection((0, 0));
        state.end_selection();
        state.copy_selection();
        assert!(state.has_region_clipboard());

        // Navigation clears the marquee but keeps the clipboard (copy A, paste on B).
        state.select_code(0x42);
        assert!(state.pixel_selection().is_none());
        assert!(state.has_region_clipboard());
    }

    #[test]
    fn rotate_selection_turns_the_region_and_is_one_undo_entry() {
        let mut state = editable_state();
        state.begin_stroke((0, 0)); // paint (0,0); the starter 'A' already has (1,1) on
        state.commit_stroke();
        assert!(state.selected_pixel(0, 0));
        assert!(state.selected_pixel(1, 1));
        assert!(!state.selected_pixel(1, 0));
        assert!(!state.selected_pixel(0, 1));

        // Rotate the 2×2 region [0..1]×[0..1] clockwise: (0,0)&(1,1) → (1,0)&(0,1).
        state.begin_selection((0, 0));
        state.extend_selection((1, 1));
        state.end_selection();
        state.rotate_selection();

        assert!(state.selected_pixel(1, 0));
        assert!(state.selected_pixel(0, 1));
        assert!(!state.selected_pixel(0, 0));
        assert!(!state.selected_pixel(1, 1));
        assert!(state.can_undo());

        state.undo(); // one undoable entry restores the region
        assert!(state.selected_pixel(0, 0));
        assert!(state.selected_pixel(1, 1));
    }

    #[test]
    fn shift_glyph_moves_the_current_glyph_and_is_one_undo_entry() {
        let mut state = editable_state();
        // Paint (0,0) on the otherwise 'A'-shaped starter glyph (column 0 is unused).
        state.begin_stroke((0, 0));
        state.commit_stroke();
        assert!(state.selected_pixel(0, 0));

        // Discard shift right by one: (0,0) → (1,0), and column 0 vacates.
        assert!(!state.shift_wrap);
        state.shift_glyph(1, 0);
        assert!(state.selected_pixel(1, 0));
        assert!(!state.selected_pixel(0, 0));
        assert!(state.can_undo());

        // One undo entry restores the pre-shift glyph.
        state.undo();
        assert!(state.selected_pixel(0, 0));
        assert!(!state.selected_pixel(1, 0));
    }

    #[test]
    fn shift_glyph_wrap_rotates_pixels_around_the_edge() {
        let mut state = editable_state();
        state.begin_stroke((0, 0));
        state.commit_stroke();
        assert!(state.selected_pixel(0, 0));

        // With wrap on, shifting left by one carries column 0 around to the last column.
        state.shift_wrap = true;
        state.shift_glyph(-1, 0);
        let width = state.selected_context().unwrap().0.glyph_size.width;
        assert!(state.selected_pixel(width - 1, 0));
        assert!(!state.selected_pixel(0, 0));
    }

    #[test]
    fn navigating_to_another_glyph_clears_the_marquee() {
        let mut state = editable_state();
        state.begin_selection((0, 0));
        state.extend_selection((3, 3));
        state.end_selection();
        assert!(state.pixel_selection().is_some());

        state.select_code(0x42);
        assert!(state.pixel_selection().is_none());
    }

    #[test]
    fn switching_documents_clears_the_marquee() {
        let mut state = editable_state();
        state.begin_selection((0, 0));
        state.extend_selection((3, 3));
        state.end_selection();
        assert!(state.pixel_selection().is_some());

        // Opening a second document makes it active — the marquee (tied to the first
        // document's glyph) must not leak onto it.
        state.open_document(loaded_starter(), PathBuf::from("/tmp/b.fontspace.json"));
        assert!(state.pixel_selection().is_none());
    }

    #[test]
    fn reverting_clears_the_marquee() {
        let mut state = editable_state();
        state.mark_saved(PathBuf::from("/tmp/a.fontspace.json"));
        state.begin_selection((0, 0));
        state.extend_selection((3, 3));
        state.end_selection();
        assert!(state.pixel_selection().is_some());

        state.load_document(loaded_starter(), PathBuf::from("/tmp/a.fontspace.json"));
        assert!(state.pixel_selection().is_none());
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

        // Redo re-applies on the originating document: the erase returns.
        state.redo();
        assert!(!state.selected_pixel(2, 0));
    }

    #[test]
    fn reverting_a_document_keeps_other_documents_undo_history() {
        // The §11.6 invariant: Revert drops only the reverted document's undo entries,
        // leaving other open documents' history intact.
        let mut state = editable_state();
        let first_id = state.open_documents().next().unwrap().0.id;
        state.mark_saved(PathBuf::from("/tmp/a.fontspace.json"));
        state.begin_stroke((0, 0)); // an edit on the first document
        state.commit_stroke();

        // Open a second document and give it an edit too.
        state.open_document(loaded_starter(), PathBuf::from("/tmp/b.fontspace.json"));
        state.begin_stroke((0, 0));
        state.commit_stroke();
        assert_eq!(state.undo_stack.len(), 2); // one entry per document

        // Revert the first document (reload in place).
        state.switch_to(first_id);
        state.load_document(loaded_starter(), PathBuf::from("/tmp/a.fontspace.json"));

        // Only the first document's entry was dropped; the second's survives.
        assert_eq!(state.undo_stack.len(), 1);
    }

    #[test]
    fn closing_is_only_possible_with_another_document_open() {
        let mut state = editable_state();
        // A single document cannot be closed — the workspace keeps at least one.
        assert!(!state.can_close());
        state.open_document(loaded_starter(), PathBuf::from("/tmp/b.fontspace.json"));
        assert!(state.can_close());
    }

    #[test]
    fn closing_the_only_document_is_a_no_op() {
        let mut state = editable_state();
        let only_id = state.open_documents().next().unwrap().0.id;
        state.close_active();
        assert_eq!(state.open_document_count(), 1);
        assert_eq!(state.open_documents().next().unwrap().0.id, only_id);
    }

    #[test]
    fn closing_promotes_the_background_document_and_drops_its_undo_history() {
        let mut state = editable_state();
        let first_id = state.open_documents().next().unwrap().0.id;
        state.begin_stroke((0, 0)); // an edit on the first document
        state.commit_stroke();

        // Open a second document and give it an edit; now two entries, b active.
        state.open_document(loaded_starter(), PathBuf::from("/tmp/b.fontspace.json"));
        let second_id = state.open_documents().next().unwrap().0.id;
        state.begin_stroke((0, 0));
        state.commit_stroke();
        assert_eq!(state.undo_stack.len(), 2); // one entry per document

        // Close the active (second) document: the first is promoted back to active,
        // and only the closed document's undo entry is dropped.
        state.close_active();
        assert_eq!(state.open_document_count(), 1);
        assert_eq!(state.open_documents().next().unwrap().0.id, first_id);
        assert_eq!(state.undo_stack.len(), 1); // b's entry gone, a's survives
        assert!(state.can_undo()); // a's edit is still undoable

        // The closed document's id is truly gone.
        assert!(state.document_mut_by_id(second_id).is_none());
    }

    #[test]
    fn closing_a_dirty_document_arms_the_confirmation() {
        let mut state = editable_state();
        state.open_document(loaded_starter(), PathBuf::from("/tmp/b.fontspace.json"));
        state.begin_stroke((0, 0)); // make the active document dirty
        state.commit_stroke();

        // A guarded Close on a dirty document does not proceed; it arms the modal.
        assert!(!state.begin_guarded(GuardedIntent::Close));
        assert_eq!(state.pending_discard(), Some(GuardedIntent::Close));
        assert_eq!(state.open_document_count(), 2); // nothing closed yet
    }
}
