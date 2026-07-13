//! Change sets: the invertible record of one operation, and the undo/redo unit
//! (spec/07 §7.7).
//!
//! Slices 4a–4b cover glyph, page, and guide edits. The character-set and export
//! change variants join [`ObjectChange`] in the later operation slices.

use fontspace_model::{Bitmap, GlyphPage, GlyphSetId, Guide, GuideId, PageId};

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
    PageInserted(PageChange),
    PageRemoved(PageChange),
    PagesReordered(PagesReorder),
    GuideChanged(GuideChange),
}

impl ObjectChange {
    fn inverted(&self) -> ObjectChange {
        match self {
            ObjectChange::GlyphChanged(change) => ObjectChange::GlyphChanged(change.inverted()),
            // Insert and remove are each other's inverse (same index + page).
            ObjectChange::PageInserted(change) => ObjectChange::PageRemoved(change.clone()),
            ObjectChange::PageRemoved(change) => ObjectChange::PageInserted(change.clone()),
            ObjectChange::PagesReordered(change) => ObjectChange::PagesReordered(change.inverted()),
            ObjectChange::GuideChanged(change) => ObjectChange::GuideChanged(change.inverted()),
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

/// A page inserted into or removed from a glyph set at a specific index. Insert and
/// remove share this type — they are each other's inverse (spec/07 §7.7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageChange {
    pub glyph_set_id: GlyphSetId,
    pub index: usize,
    pub page: GlyphPage,
}

/// A reordering of a glyph set's pages from `before` to `after` (each a permutation
/// of the same page ids).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PagesReorder {
    pub glyph_set_id: GlyphSetId,
    pub before: Vec<PageId>,
    pub after: Vec<PageId>,
}

impl PagesReorder {
    fn inverted(&self) -> PagesReorder {
        PagesReorder {
            glyph_set_id: self.glyph_set_id,
            before: self.after.clone(),
            after: self.before.clone(),
        }
    }
}

/// A guide changing on a page. `before = None` is an add, `after = None` a remove,
/// both `Some` a move/edit. Guides are identified by `guide_id`; a copy to another
/// page mints a fresh id (spec/03 §3.8), so this always concerns one guide identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuideChange {
    pub glyph_set_id: GlyphSetId,
    pub page_id: PageId,
    pub guide_id: GuideId,
    pub before: Option<Guide>,
    pub after: Option<Guide>,
}

impl GuideChange {
    fn inverted(&self) -> GuideChange {
        GuideChange {
            glyph_set_id: self.glyph_set_id,
            page_id: self.page_id,
            guide_id: self.guide_id,
            before: self.after.clone(),
            after: self.before.clone(),
        }
    }
}

/// A non-fatal warning attached to a change set (spec/07 §7.7). Uninhabited through
/// slice 4b — glyph/page/guide edits never warn; the recode-orphan and remove-cascade
/// warnings arrive with the character-set operation slice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FontSpaceWarning {}
