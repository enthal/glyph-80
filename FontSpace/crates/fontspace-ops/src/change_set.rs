//! Change sets: the invertible record of one operation, and the undo/redo unit
//! (spec/07 §7.7).
//!
//! Slice 4a covers pixel/glyph edits, so [`ObjectChange`] has only the
//! [`GlyphChanged`](ObjectChange::GlyphChanged) variant today; page, guide,
//! character-set, and export changes join it in the later operation slices.

use fontspace_model::{Bitmap, GlyphSetId, PageId};

/// The invertible result of one operation. Empty when the operation was a no-op.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ChangeSet {
    pub object_changes: Vec<ObjectChange>,
    pub warnings: Vec<FontSpaceWarning>,
}

impl ChangeSet {
    pub fn is_empty(&self) -> bool {
        self.object_changes.is_empty()
    }

    /// The exact inverse: applying `cs` then `cs.invert()` restores the prior state.
    /// Changes are reversed in order (so overlapping edits undo correctly) and each
    /// is individually inverted. Warnings describe the forward action and are dropped.
    pub fn invert(&self) -> ChangeSet {
        ChangeSet {
            object_changes: self
                .object_changes
                .iter()
                .rev()
                .map(ObjectChange::inverted)
                .collect(),
            warnings: Vec::new(),
        }
    }
}

/// One reversible change to a document object (spec/07 §7.7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObjectChange {
    GlyphChanged(GlyphChange),
}

impl ObjectChange {
    fn inverted(&self) -> ObjectChange {
        match self {
            ObjectChange::GlyphChanged(change) => ObjectChange::GlyphChanged(change.inverted()),
        }
    }
}

/// A glyph's bitmap changing from `before` to `after`. `before` is blank if the
/// glyph did not previously exist; `after` blank if it was cleared (spec/07 §7.7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlyphChange {
    pub glyph_set_id: GlyphSetId,
    pub page_id: PageId,
    pub code: u32,
    pub before: Bitmap,
    pub after: Bitmap,
}

impl GlyphChange {
    fn inverted(&self) -> GlyphChange {
        GlyphChange {
            glyph_set_id: self.glyph_set_id,
            page_id: self.page_id,
            code: self.code,
            before: self.after.clone(),
            after: self.before.clone(),
        }
    }
}

/// A non-fatal warning attached to a change set (spec/07 §7.7). Uninhabited in
/// slice 4a — glyph edits never warn; the recode-orphan and remove-cascade warnings
/// arrive with the character-set operation slice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FontSpaceWarning {}
