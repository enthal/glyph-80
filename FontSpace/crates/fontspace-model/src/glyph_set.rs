//! Glyph sets, pages, glyphs, and guides (spec/03 §3.5–3.8).
//!
//! A glyph set fixes one geometry, references one character set, and holds pages.
//! Pages are **sparse**: a page stores a [`Glyph`] only for the codes it defines;
//! absent codes render blank (spec/03 §3.6, spec/05 §5.6). Glyphs are keyed by the
//! entry `code` they render.

use crate::{Bitmap, CharacterSetId, GlyphSetId, GlyphSize, GuideAxis, GuideId, IdGen, PageId};

/// The on/off pixels rendered for a `code` on a page. The bitmap's size must equal
/// the owning glyph set's `glyph_size` (geometry agreement, spec/05 §5.5;
/// checked by document validation, spec/14).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Glyph {
    pub code: u32,
    pub bitmap: Bitmap,
}

/// A named integer coordinate on a page (baseline, cap height, …). `position` is a
/// grid-line coordinate between pixels, signed, and may lie outside glyph bounds
/// (spec/03 §3.8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Guide {
    pub id: GuideId,
    pub name: String,
    pub axis: GuideAxis,
    pub position: i32,
    pub visible: bool,
    pub locked: bool,
}

/// A sparse set of glyphs within a glyph set, plus its guides. Canonical glyph
/// order is charset-entry order (spec/03 §3.6); this in-memory `glyphs` vector is
/// looked up by `code` and reconciled to canonical order on save (spec/06).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlyphPage {
    pub id: PageId,
    pub name: String,
    pub description: String,
    pub guides: Vec<Guide>,
    pub glyphs: Vec<Glyph>,
}

impl GlyphPage {
    /// A new, empty page (no guides, no glyphs) with a freshly-minted id.
    pub fn new(
        ids: &mut dyn IdGen,
        name: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        Self {
            id: PageId::new(ids),
            name: name.into(),
            description: description.into(),
            guides: Vec::new(),
            glyphs: Vec::new(),
        }
    }

    /// The glyph rendering `code` on this page, if one is stored.
    pub fn glyph_by_code(&self, code: u32) -> Option<&Glyph> {
        self.glyphs.iter().find(|glyph| glyph.code == code)
    }

    /// Mutable access to the glyph rendering `code`, if one is stored.
    pub fn glyph_by_code_mut(&mut self, code: u32) -> Option<&mut Glyph> {
        self.glyphs.iter_mut().find(|glyph| glyph.code == code)
    }
}

/// One geometry + one character-set reference + pages. Multiple glyph sets of
/// different geometry may reference the same character set (spec/03 §3.5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlyphSet {
    pub id: GlyphSetId,
    pub name: String,
    pub description: String,
    pub glyph_size: GlyphSize,
    pub character_set_id: CharacterSetId,
    pub pages: Vec<GlyphPage>,
}

impl GlyphSet {
    /// A new, empty glyph set (no pages) with a freshly-minted id.
    pub fn new(
        ids: &mut dyn IdGen,
        name: impl Into<String>,
        description: impl Into<String>,
        glyph_size: GlyphSize,
        character_set_id: CharacterSetId,
    ) -> Self {
        Self {
            id: GlyphSetId::new(ids),
            name: name.into(),
            description: description.into(),
            glyph_size,
            character_set_id,
            pages: Vec::new(),
        }
    }

    /// The page with `id`, if present.
    pub fn page_by_id(&self, id: PageId) -> Option<&GlyphPage> {
        self.pages.iter().find(|page| page.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SequentialIdGen;

    #[test]
    fn glyph_by_code_finds_sparse_entries() {
        let mut ids = SequentialIdGen::new();
        let mut page = GlyphPage::new(&mut ids, "page", "");
        page.glyphs.push(Glyph {
            code: 0x41,
            bitmap: Bitmap::new_blank(GlyphSize::new(8, 8)),
        });
        assert_eq!(page.glyph_by_code(0x41).map(|g| g.code), Some(0x41));
        assert!(page.glyph_by_code(0x42).is_none()); // absent = blank
        page.glyph_by_code_mut(0x41).unwrap().code = 0x41;
    }
}
