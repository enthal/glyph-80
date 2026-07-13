//! Text-grid rendering tests (spec/09, spec/15 §15.3 expected text renderings).
//! Pure functions with fixed input, so outputs are deterministic and asserted exactly.

use fontspace_model::{
    Bitmap, CharacterEntry, CharacterSet, FontSpace, Glyph, GlyphPage, GlyphSet, GlyphSetId,
    GlyphSize, SequentialIdGen,
};
use fontspace_ops::{FontSpaceError, GlyphSelector, PageSelector};

use crate::{RenderLayout, TextGridRequest, render_text_grid};

/// A 3×3 glyph set over codes 0x41/0x42, with 0x41 drawn as an X-ish pattern
/// (`#.#` / `.#.` / `#.#`) on the one page; 0x42 has no glyph (renders blank).
fn fixture() -> (FontSpace, GlyphSetId) {
    let mut ids = SequentialIdGen::new();
    let mut character_set = CharacterSet::new(&mut ids, "cs", "");
    character_set.entries = vec![
        CharacterEntry {
            code: 0x41,
            label: "A".into(),
        },
        CharacterEntry {
            code: 0x42,
            label: "B".into(),
        },
    ];
    let size = GlyphSize::new(3, 3);
    let mut glyph_set = GlyphSet::new(&mut ids, "gs", "", size, character_set.id);
    let mut page = GlyphPage::new(&mut ids, "Regular", "");
    let mut a = Bitmap::new_blank(size);
    for (x, y) in [(0, 0), (2, 0), (1, 1), (0, 2), (2, 2)] {
        a.set(x, y, true).unwrap();
    }
    page.glyphs.push(Glyph {
        code: 0x41,
        bitmap: a,
    });
    glyph_set.pages.push(page);

    let glyph_set_id = glyph_set.id;
    let mut doc = FontSpace::new(&mut ids, "doc", "");
    doc.character_sets.push(character_set);
    doc.glyph_sets.push(glyph_set);
    (doc, glyph_set_id)
}

fn base(glyph_set: GlyphSetId, glyphs: GlyphSelector, layout: RenderLayout) -> TextGridRequest {
    TextGridRequest {
        glyph_set_id: glyph_set,
        pages: PageSelector::All,
        glyphs,
        on: "#".into(),
        off: ".".into(),
        glyph_separator: "|".into(),
        row_separator: "\n".into(),
        page_separator: "\n--\n".into(),
        layout,
        scale_x: 1,
        scale_y: 1,
    }
}

#[test]
fn single_glyph_renders_visual_rows() {
    let (doc, gs) = fixture();
    let req = base(
        gs,
        GlyphSelector::Code(0x41),
        RenderLayout::GlyphsHorizontal,
    );
    assert_eq!(render_text_grid(&doc, &req).unwrap(), "#.#\n.#.\n#.#");
}

#[test]
fn scale_repeats_tokens_per_pixel() {
    let (doc, gs) = fixture();
    let mut req = base(
        gs,
        GlyphSelector::Code(0x41),
        RenderLayout::GlyphsHorizontal,
    );
    req.scale_x = 2;
    // Each pixel token doubled horizontally; rows unchanged in count (scale_y = 1).
    assert_eq!(
        render_text_grid(&doc, &req).unwrap(),
        "##..##\n..##..\n##..##"
    );
}

#[test]
fn arbitrary_on_off_tokens() {
    let (doc, gs) = fixture();
    let mut req = base(
        gs,
        GlyphSelector::Code(0x41),
        RenderLayout::GlyphsHorizontal,
    );
    req.on = "@@".into();
    req.off = "..".into();
    assert_eq!(
        render_text_grid(&doc, &req).unwrap(),
        "@@..@@\n..@@..\n@@..@@"
    );
}

#[test]
fn absent_code_renders_blank() {
    let (doc, gs) = fixture();
    // 0x42 has no glyph on the page → all off.
    let req = base(
        gs,
        GlyphSelector::Code(0x42),
        RenderLayout::GlyphsHorizontal,
    );
    assert_eq!(render_text_grid(&doc, &req).unwrap(), "...\n...\n...");
}

#[test]
fn glyphs_horizontal_joins_with_glyph_separator() {
    let (doc, gs) = fixture();
    // Codes 0x41 (drawn) and 0x42 (blank) side by side, separated by '|'.
    let req = base(gs, GlyphSelector::All, RenderLayout::GlyphsHorizontal);
    assert_eq!(
        render_text_grid(&doc, &req).unwrap(),
        "#.#|...\n.#.|...\n#.#|..."
    );
}

#[test]
fn glyphs_vertical_stacks_with_separator_row() {
    let (doc, gs) = fixture();
    let mut req = base(gs, GlyphSelector::All, RenderLayout::GlyphsVertical);
    req.glyph_separator = "~~~".into();
    assert_eq!(
        render_text_grid(&doc, &req).unwrap(),
        "#.#\n.#.\n#.#\n~~~\n...\n...\n..."
    );
}

#[test]
fn grid_wraps_into_columns() {
    let (doc, gs) = fixture();
    // Two glyphs, 1 column => stacked, joined by the glyph separator row.
    let mut req = base(gs, GlyphSelector::All, RenderLayout::Grid { columns: 1 });
    req.glyph_separator = "==".into();
    assert_eq!(
        render_text_grid(&doc, &req).unwrap(),
        "#.#\n.#.\n#.#\n==\n...\n...\n..."
    );
}

#[test]
fn scale_y_repeats_rows() {
    let (doc, gs) = fixture();
    let mut req = base(
        gs,
        GlyphSelector::Code(0x41),
        RenderLayout::GlyphsHorizontal,
    );
    req.scale_y = 2;
    assert_eq!(
        render_text_grid(&doc, &req).unwrap(),
        "#.#\n#.#\n.#.\n.#.\n#.#\n#.#"
    );
}

/// A glyph set with two pages: Regular draws 0x41, Bold draws 0x42 (the top row),
/// so page-level layouts have visibly different pages.
fn two_page_fixture() -> (FontSpace, GlyphSetId) {
    let mut ids = SequentialIdGen::new();
    let mut character_set = CharacterSet::new(&mut ids, "cs", "");
    character_set.entries = vec![
        CharacterEntry {
            code: 0x41,
            label: "A".into(),
        },
        CharacterEntry {
            code: 0x42,
            label: "B".into(),
        },
    ];
    let size = GlyphSize::new(3, 3);
    let mut glyph_set = GlyphSet::new(&mut ids, "gs", "", size, character_set.id);

    let mut regular = GlyphPage::new(&mut ids, "Regular", "");
    let mut a = Bitmap::new_blank(size);
    for (x, y) in [(0, 0), (2, 0), (1, 1), (0, 2), (2, 2)] {
        a.set(x, y, true).unwrap();
    }
    regular.glyphs.push(Glyph {
        code: 0x41,
        bitmap: a,
    });

    let mut bold = GlyphPage::new(&mut ids, "Bold", "");
    let mut b = Bitmap::new_blank(size);
    for x in 0..3 {
        b.set(x, 0, true).unwrap(); // top row on
    }
    bold.glyphs.push(Glyph {
        code: 0x42,
        bitmap: b,
    });

    glyph_set.pages.push(regular);
    glyph_set.pages.push(bold);

    let glyph_set_id = glyph_set.id;
    let mut doc = FontSpace::new(&mut ids, "doc", "");
    doc.character_sets.push(character_set);
    doc.glyph_sets.push(glyph_set);
    (doc, glyph_set_id)
}

#[test]
fn pages_vertical_stacks_pages_with_page_separator() {
    let (doc, gs) = two_page_fixture();
    // Render 0x41 on both pages: Regular has it (X), Bold does not (blank).
    let mut req = base(gs, GlyphSelector::Code(0x41), RenderLayout::PagesVertical);
    req.page_separator = "==".into();
    assert_eq!(
        render_text_grid(&doc, &req).unwrap(),
        "#.#\n.#.\n#.#\n==\n...\n...\n..."
    );
}

#[test]
fn pages_horizontal_places_pages_side_by_side() {
    let (doc, gs) = two_page_fixture();
    let mut req = base(gs, GlyphSelector::Code(0x41), RenderLayout::PagesHorizontal);
    req.page_separator = " || ".into();
    assert_eq!(
        render_text_grid(&doc, &req).unwrap(),
        "#.# || ...\n.#. || ...\n#.# || ..."
    );
}

#[test]
fn grid_wraps_after_columns() {
    // Three codes, 2 columns => first row [0x41, 0x42], second row [0x43].
    let mut ids = SequentialIdGen::new();
    let mut character_set = CharacterSet::new(&mut ids, "cs", "");
    character_set.entries = vec![
        CharacterEntry {
            code: 0x41,
            label: "A".into(),
        },
        CharacterEntry {
            code: 0x42,
            label: "B".into(),
        },
        CharacterEntry {
            code: 0x43,
            label: "C".into(),
        },
    ];
    let size = GlyphSize::new(1, 1);
    let mut glyph_set = GlyphSet::new(&mut ids, "gs", "", size, character_set.id);
    let mut page = GlyphPage::new(&mut ids, "Regular", "");
    // Draw 0x41 and 0x43 (on), leave 0x42 blank (off), so the row content differs.
    for code in [0x41u32, 0x43] {
        let mut bmp = Bitmap::new_blank(size);
        bmp.set(0, 0, true).unwrap();
        page.glyphs.push(Glyph { code, bitmap: bmp });
    }
    glyph_set.pages.push(page);
    let gs = glyph_set.id;
    let mut doc = FontSpace::new(&mut ids, "doc", "");
    doc.character_sets.push(character_set);
    doc.glyph_sets.push(glyph_set);

    let mut req = base(gs, GlyphSelector::All, RenderLayout::Grid { columns: 2 });
    req.glyph_separator = "|".into();
    // Row of glyphs [# , .] then wrap to [#]; glyph rows joined by the separator too.
    assert_eq!(render_text_grid(&doc, &req).unwrap(), "#|.\n|\n#");
}

#[test]
fn unknown_glyph_set_errors() {
    let (doc, _gs) = fixture();
    let mut ids = SequentialIdGen::new();
    let bogus = GlyphSet::new(&mut ids, "x", "", GlyphSize::new(1, 1), {
        // a character-set id we won't use; only the glyph-set id matters here
        let mut ids2 = SequentialIdGen::new();
        CharacterSet::new(&mut ids2, "y", "").id
    })
    .id;
    let req = base(bogus, GlyphSelector::All, RenderLayout::GlyphsHorizontal);
    assert!(matches!(
        render_text_grid(&doc, &req).unwrap_err(),
        FontSpaceError::GlyphSetNotFound(_)
    ));
}

// --- text-string rendering (spec/09 §9.2.1) -------------------------------

use crate::{TextStringRequest, render_text_string};

/// A text-string request over the 3×3 fixture: `on`/`off` = `#`/`.`, no glyph
/// separator (contiguous text), one page.
fn text(glyph_set: GlyphSetId, rows: Vec<Vec<u32>>) -> TextStringRequest {
    TextStringRequest {
        glyph_set_id: glyph_set,
        pages: PageSelector::All,
        rows,
        on: "#".into(),
        off: ".".into(),
        glyph_separator: String::new(),
        row_separator: "\n".into(),
        page_separator: "\n--\n".into(),
        scale_x: 1,
        scale_y: 1,
    }
}

#[test]
fn text_string_repeats_a_glyph() {
    let (doc, gs) = fixture();
    // 0x41 twice, side by side (X-pattern: #.# / .#. / #.#).
    let req = text(gs, vec![vec![0x41, 0x41]]);
    assert_eq!(
        render_text_string(&doc, &req).unwrap(),
        "#.##.#\n.#..#.\n#.##.#"
    );
}

#[test]
fn text_string_ignores_codes_with_no_charset_entry() {
    let (doc, gs) = fixture();
    // 0x43 has no entry → ignored; result equals rendering just the two 0x41s.
    let with_unknown = render_text_string(&doc, &text(gs, vec![vec![0x41, 0x43, 0x41]])).unwrap();
    let without = render_text_string(&doc, &text(gs, vec![vec![0x41, 0x41]])).unwrap();
    assert_eq!(with_unknown, without);
}

#[test]
fn text_string_renders_entry_without_glyph_as_blank() {
    let (doc, gs) = fixture();
    // 0x42 has an entry but no stored glyph → a blank 3×3 cell (not skipped).
    let req = text(gs, vec![vec![0x42]]);
    assert_eq!(render_text_string(&doc, &req).unwrap(), "...\n...\n...");
}

#[test]
fn text_string_newline_starts_a_new_line() {
    let (doc, gs) = fixture();
    // Two lines of one 0x41 each → the second X stacks directly below the first.
    let req = text(gs, vec![vec![0x41], vec![0x41]]);
    assert_eq!(
        render_text_string(&doc, &req).unwrap(),
        "#.#\n.#.\n#.#\n#.#\n.#.\n#.#"
    );
}

#[test]
fn text_string_empty_line_contributes_nothing() {
    let (doc, gs) = fixture();
    // An all-unknown (or empty) line adds no rows.
    let req = text(gs, vec![vec![0x41], vec![], vec![0x41]]);
    assert_eq!(
        render_text_string(&doc, &req).unwrap(),
        "#.#\n.#.\n#.#\n#.#\n.#.\n#.#"
    );
}

#[test]
fn text_string_stacks_pages_with_page_separator() {
    let (mut doc, gs) = fixture();
    // Add a second, empty page: 0x41 renders blank there.
    let mut ids = SequentialIdGen::new();
    doc.glyph_sets[0]
        .pages
        .push(GlyphPage::new(&mut ids, "Blank", ""));
    let mut req = text(gs, vec![vec![0x41]]);
    req.page_separator = "--".into(); // single-line separator for a clean assertion
    // Page "Regular" (X pattern), separator, page "Blank" (3×3 blank).
    assert_eq!(
        render_text_string(&doc, &req).unwrap(),
        "#.#\n.#.\n#.#\n--\n...\n...\n..."
    );
}

#[test]
fn text_string_with_no_character_set_renders_nothing() {
    let (mut doc, gs) = fixture();
    // Drop the character set the glyph set references (a dangling reference): with
    // no entries, every code is un-gated and ignored, so the render is empty.
    doc.character_sets.clear();
    let req = text(gs, vec![vec![0x41, 0x42]]);
    assert_eq!(render_text_string(&doc, &req).unwrap(), "");
}
