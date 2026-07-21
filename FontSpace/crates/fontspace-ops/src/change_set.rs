//! Change sets: the invertible record of one operation, and the undo/redo unit
//! (spec/07 §7.7).
//!
//! Slices 4a–4b cover glyph, page, and guide edits. The character-set and export
//! change variants join [`ObjectChange`] in the later operation slices.

use fontspace_model::{
    Bitmap, CharacterEntry, CharacterSetId, ExportConfig, GlyphPage, GlyphSet, GlyphSetId, Guide,
    GuideId, PageId,
};

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
    CharacterSetChanged(CharacterSetChange),
    /// A whole glyph removed from a page (e.g. the entry-removal cascade, spec/07 §7.8).
    GlyphRemoved(GlyphPlacement),
    /// A whole glyph inserted at an index — the inverse of `GlyphRemoved`.
    GlyphInserted(GlyphPlacement),
    /// A top-level glyph set added, removed, or replaced (spec/07 §7.7). `before = None`
    /// is an add, `after = None` a remove, both `Some` a replace. Boxed: a whole glyph
    /// set is far larger than the other variants (it carries every page and glyph).
    GlyphSetChanged(Box<GlyphSetChange>),
    /// A top-level export config added, removed, or replaced (spec/07 §7.7). Same
    /// add/remove/replace convention as [`GlyphSetChange`]; boxed for the same reason.
    ExportConfigChanged(Box<ExportConfigChange>),
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
            ObjectChange::CharacterSetChanged(change) => {
                ObjectChange::CharacterSetChanged(change.inverted())
            }
            ObjectChange::GlyphRemoved(placement) => ObjectChange::GlyphInserted(placement.clone()),
            ObjectChange::GlyphInserted(placement) => ObjectChange::GlyphRemoved(placement.clone()),
            ObjectChange::GlyphSetChanged(change) => {
                ObjectChange::GlyphSetChanged(Box::new(change.inverted()))
            }
            ObjectChange::ExportConfigChanged(change) => {
                ObjectChange::ExportConfigChanged(Box::new(change.inverted()))
            }
        }
    }
}

/// A top-level glyph set added, removed, or replaced (spec/07 §7.7). `before = None`
/// is an add, `after = None` a remove, both `Some` a replace/edit. `index` is the
/// glyph set's position in the document's `glyph_sets` vector, so re-insert (an add,
/// or the undo of a remove) restores it exactly — glyph-set order is document
/// identity (it round-trips through canonical JSON). Add/remove/edit are matched by
/// the glyph set's own `id`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlyphSetChange {
    pub index: usize,
    pub before: Option<GlyphSet>,
    pub after: Option<GlyphSet>,
}

impl GlyphSetChange {
    fn inverted(&self) -> GlyphSetChange {
        GlyphSetChange {
            index: self.index,
            before: self.after.clone(),
            after: self.before.clone(),
        }
    }
}

/// A top-level export config added, removed, or replaced (spec/07 §7.7). Same
/// `index` / `before` / `after` convention as [`GlyphSetChange`]; matched by the
/// config's own `id`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportConfigChange {
    pub index: usize,
    pub before: Option<ExportConfig>,
    pub after: Option<ExportConfig>,
}

impl ExportConfigChange {
    fn inverted(&self) -> ExportConfigChange {
        ExportConfigChange {
            index: self.index,
            before: self.after.clone(),
            after: self.before.clone(),
        }
    }
}

/// A whole glyph at a specific position in a page's glyph vector. Records the index
/// so a removed glyph is restored exactly (spec/07 §7.8). Distinct from `GlyphChange`,
/// which edits an existing glyph's bitmap in place; this adds or removes the glyph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlyphPlacement {
    pub glyph_set_id: GlyphSetId,
    pub page_id: PageId,
    pub index: usize,
    pub code: u32,
    pub bitmap: Bitmap,
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
///
/// `index` is the guide's position in the page's `guides` vector. On re-insert (an
/// add, or the undo of a remove) the guide is placed **at `index`**, so inversion is
/// exact even when a page has several guides (spec/07 §7.7) — guide order is part of
/// document identity (it round-trips through canonical JSON).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuideChange {
    pub glyph_set_id: GlyphSetId,
    pub page_id: PageId,
    pub guide_id: GuideId,
    pub index: usize,
    pub before: Option<Guide>,
    pub after: Option<Guide>,
}

impl GuideChange {
    fn inverted(&self) -> GuideChange {
        GuideChange {
            glyph_set_id: self.glyph_set_id,
            page_id: self.page_id,
            guide_id: self.guide_id,
            index: self.index,
            before: self.after.clone(),
            after: self.before.clone(),
        }
    }
}

/// A character set's ordered entries changing from `before` to `after`. Every
/// entry-list edit (add/reorder/recode now, relabel and cascade-remove later)
/// reuses it — the list mutation is captured whole (spec/04 §4.4, spec/07 §7.7);
/// the cascade-remove additionally emits glyph changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharacterSetChange {
    pub character_set_id: CharacterSetId,
    pub before: Vec<CharacterEntry>,
    pub after: Vec<CharacterEntry>,
}

impl CharacterSetChange {
    fn inverted(&self) -> CharacterSetChange {
        CharacterSetChange {
            character_set_id: self.character_set_id,
            before: self.after.clone(),
            after: self.before.clone(),
        }
    }
}

/// A glyph left dangling (its `code` no longer has an entry) by a recode — reported,
/// never moved or deleted (spec/04 §4.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrphanedGlyph {
    pub glyph_set_id: GlyphSetId,
    pub page_id: PageId,
    pub code: u32,
}

/// A glyph deleted by an entry-removal cascade (spec/07 §7.8). Listed in the
/// warning so the user sees exactly what was removed; undo restores it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CascadedGlyph {
    pub glyph_set_id: GlyphSetId,
    pub page_id: PageId,
    pub code: u32,
}

/// A non-fatal warning attached to a change set (spec/07 §7.7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FontSpaceWarning {
    /// A `RecodeCharacterEntry` changed an entry's `code`, leaving glyphs that still
    /// reference the old code dangling. They are listed, not moved (spec/04 §4.4).
    RecodeOrphanedGlyphs {
        character_set_id: CharacterSetId,
        from_code: u32,
        to_code: u32,
        orphaned: Vec<OrphanedGlyph>,
    },
    /// A `RemoveCharacterEntry` cascade-deleted the listed glyphs across every glyph
    /// set referencing the character set (spec/04 §4.4, spec/07 §7.8). Undo restores
    /// the entry and every glyph.
    RemoveCascade {
        character_set_id: CharacterSetId,
        code: u32,
        removed: Vec<CascadedGlyph>,
    },
}
