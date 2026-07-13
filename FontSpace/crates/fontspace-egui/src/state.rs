//! Application document state: the in-memory `FontSpace` the GUI edits, the current
//! selection, and editor view options.
//!
//! Persistence (open/save) and the multi-document workspace arrive in later slices
//! (spec/11); until then the app opens a small in-memory starter document so the
//! editor and other views have real content to show. The GUI owns this state but
//! **not** font semantics — mutations go through `fontspace-ops` (spec/02).

use fontspace_model::{
    Bitmap, CharacterEntry, CharacterSet, FontSpace, Glyph, GlyphPage, GlyphSet, GlyphSetId,
    GlyphSize, Guide, GuideAxis, IdGen, PageId, RandomIdGen,
};

use crate::editor::geometry::GridLevel;

/// What the editor and inspector are currently pointed at: one glyph, identified by
/// its glyph set, page, and character `code` (spec/12 §12.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub glyph_set_id: GlyphSetId,
    pub page_id: PageId,
    pub code: u32,
}

/// The whole editable state of the (single) open document plus view options.
pub struct AppState {
    pub document: FontSpace,
    /// Injected id source for object-creating operations (spec/03 §Id injection).
    pub ids: Box<dyn IdGen>,
    pub selection: Selection,
    pub grid: GridLevel,
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
        let (document, selection) = starter_document(ids.as_mut());
        Self {
            document,
            ids,
            selection,
            grid: GridLevel::Subtle,
        }
    }

    /// The selected glyph set and page, if the selection still resolves.
    pub fn selected_context(&self) -> Option<(&GlyphSet, &GlyphPage)> {
        let glyph_set = self.document.glyph_set(self.selection.glyph_set_id)?;
        let page = glyph_set.page_of_id(self.selection.page_id)?;
        Some((glyph_set, page))
    }

    /// The bitmap of the selected glyph, if one is stored (absent → blank in the UI).
    pub fn selected_bitmap(&self) -> Option<&Bitmap> {
        let (_, page) = self.selected_context()?;
        page.glyph_of_code(self.selection.code).map(|g| &g.bitmap)
    }

    /// The label of the selected code in the glyph set's character set, if any.
    pub fn selected_label(&self) -> Option<&str> {
        let glyph_set = self.document.glyph_set(self.selection.glyph_set_id)?;
        let character_set = self.document.character_set(glyph_set.character_set_id)?;
        character_set
            .entry(self.selection.code)
            .map(|entry| entry.label.as_str())
    }
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
        assert_eq!(state.selection.code, 0x41);
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
}
