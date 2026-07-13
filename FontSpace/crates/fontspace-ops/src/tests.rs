//! Operation tests (spec/07, spec/15 §15.1/§15.4). Strict-layer focus: atomicity
//! and exact change-set inversion (undo). All ids from `SequentialIdGen`.

use fontspace_model::{
    Bitmap, CharacterEntry, CharacterSet, FontSpace, Glyph, GlyphPage, GlyphSet, GlyphSetId,
    GlyphSize, GuideAxis, OverflowPolicy, PageId, SequentialIdGen,
};
use proptest::prelude::*;

use crate::{
    AddGuide, AddPage, ClearGlyphs, CopyGuideToPages, FontSpaceError, GlyphRef, GlyphSelector,
    InvertGlyphs, MoveGuide, PageSelector, PixelEdit, RemovePages, ReorderPages, SetPixels,
    ShiftGlyphs, add_guide, add_page, clear_glyphs, copy_guide_to_pages, invert_glyphs, move_guide,
    remove_pages, reorder_pages, set_pixels, shift_glyphs, undo,
};

struct Fixture {
    doc: FontSpace,
    glyph_set: GlyphSetId,
    regular: PageId,
    bold: PageId,
    ids: SequentialIdGen,
}

/// A glyph set over codes 0x41/0x42/0x43, two pages ("Regular", "Bold"), with a
/// single drawn glyph for 0x41 on the Regular page.
fn fixture() -> Fixture {
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
    let size = GlyphSize::new(8, 8);
    let mut glyph_set = GlyphSet::new(&mut ids, "gs", "", size, character_set.id);

    let mut regular = GlyphPage::new(&mut ids, "Regular", "");
    let mut a = Bitmap::new_blank(size);
    a.set(1, 1, true).unwrap();
    a.set(2, 2, true).unwrap();
    regular.glyphs.push(Glyph {
        code: 0x41,
        bitmap: a,
    });
    let bold = GlyphPage::new(&mut ids, "Bold", "");

    let regular_id = regular.id;
    let bold_id = bold.id;
    let glyph_set_id = glyph_set.id;
    glyph_set.pages.push(regular);
    glyph_set.pages.push(bold);

    let mut doc = FontSpace::new(&mut ids, "doc", "");
    doc.character_sets.push(character_set);
    doc.glyph_sets.push(glyph_set);
    Fixture {
        doc,
        glyph_set: glyph_set_id,
        regular: regular_id,
        bold: bold_id,
        ids,
    }
}

/// A document with blank glyphs pruned — the canonical form, matching save. Undo of
/// a materializing edit leaves an explicit blank glyph in memory (spec/05 §5.6), so
/// semantic/canonical equality is the right comparison after undo.
fn pruned(doc: &FontSpace) -> FontSpace {
    let mut doc = doc.clone();
    for glyph_set in &mut doc.glyph_sets {
        for page in &mut glyph_set.pages {
            page.glyphs.retain(|glyph| !glyph.bitmap.is_blank());
        }
    }
    doc
}

#[test]
fn set_pixels_materializes_and_records_blank_before() {
    let mut f = fixture();
    // 0x42 has no glyph yet on Regular.
    let req = SetPixels {
        target: GlyphRef {
            glyph_set_id: f.glyph_set,
            page_id: f.regular,
            code: 0x42,
        },
        edits: vec![PixelEdit {
            x: 3,
            y: 3,
            value: true,
        }],
    };
    let change_set = set_pixels(&mut f.doc, &req).unwrap();
    assert_eq!(change_set.object_changes.len(), 1);
    let page = &f.doc.glyph_sets[0].pages[0];
    assert!(page.glyph_of_code(0x42).unwrap().bitmap.get(3, 3).unwrap());
}

#[test]
fn set_pixels_undo_restores_semantically() {
    let mut f = fixture();
    let original = pruned(&f.doc);
    let req = SetPixels {
        target: GlyphRef {
            glyph_set_id: f.glyph_set,
            page_id: f.regular,
            code: 0x42,
        },
        edits: vec![
            PixelEdit {
                x: 3,
                y: 3,
                value: true,
            },
            PixelEdit {
                x: 4,
                y: 4,
                value: true,
            },
        ],
    };
    let change_set = set_pixels(&mut f.doc, &req).unwrap();
    undo(&mut f.doc, &change_set).unwrap();
    assert_eq!(pruned(&f.doc), original, "undo restores canonical state");
}

#[test]
fn set_pixels_out_of_bounds_is_atomic_error() {
    let mut f = fixture();
    let before = f.doc.clone();
    let req = SetPixels {
        target: GlyphRef {
            glyph_set_id: f.glyph_set,
            page_id: f.regular,
            code: 0x41,
        },
        // second edit is out of bounds for 8×8
        edits: vec![
            PixelEdit {
                x: 0,
                y: 0,
                value: true,
            },
            PixelEdit {
                x: 8,
                y: 0,
                value: true,
            },
        ],
    };
    let err = set_pixels(&mut f.doc, &req).unwrap_err();
    assert!(matches!(err, FontSpaceError::PixelOutOfBounds { x: 8, .. }));
    assert_eq!(f.doc, before, "a rejected op leaves the document unchanged");
}

#[test]
fn set_pixels_noop_returns_empty_change_set() {
    let mut f = fixture();
    // (1,1) is already on for 0x41.
    let req = SetPixels {
        target: GlyphRef {
            glyph_set_id: f.glyph_set,
            page_id: f.regular,
            code: 0x41,
        },
        edits: vec![PixelEdit {
            x: 1,
            y: 1,
            value: true,
        }],
    };
    let change_set = set_pixels(&mut f.doc, &req).unwrap();
    assert!(change_set.is_empty());
}

#[test]
fn set_pixels_rejects_code_without_entry() {
    let mut f = fixture();
    let req = SetPixels {
        target: GlyphRef {
            glyph_set_id: f.glyph_set,
            page_id: f.regular,
            code: 0x99, // no entry
        },
        edits: vec![PixelEdit {
            x: 0,
            y: 0,
            value: true,
        }],
    };
    let err = set_pixels(&mut f.doc, &req).unwrap_err();
    assert!(matches!(
        err,
        FontSpaceError::CodeNotInCharacterSet { code: 0x99, .. }
    ));
}

#[test]
fn shift_clear_invert_change_and_undo() {
    for op in ["shift", "clear", "invert"] {
        let mut f = fixture();
        let original = pruned(&f.doc);
        let change_set = match op {
            "shift" => shift_glyphs(
                &mut f.doc,
                &ShiftGlyphs {
                    glyph_set_id: f.glyph_set,
                    pages: PageSelector::All,
                    glyphs: GlyphSelector::All,
                    dx: 1,
                    dy: 0,
                    overflow: OverflowPolicy::Discard,
                },
            ),
            "clear" => clear_glyphs(
                &mut f.doc,
                &ClearGlyphs {
                    glyph_set_id: f.glyph_set,
                    pages: PageSelector::All,
                    glyphs: GlyphSelector::All,
                },
            ),
            _ => invert_glyphs(
                &mut f.doc,
                &InvertGlyphs {
                    glyph_set_id: f.glyph_set,
                    pages: PageSelector::All,
                    glyphs: GlyphSelector::All,
                },
            ),
        }
        .unwrap();
        assert!(
            !change_set.is_empty(),
            "{op} must change the drawn 0x41 glyph"
        );
        undo(&mut f.doc, &change_set).unwrap();
        assert_eq!(pruned(&f.doc), original, "{op} undo restores state");
    }
}

#[test]
fn op_on_empty_page_is_a_noop() {
    // The Bold page has no glyphs, so selecting it changes nothing.
    let mut f = fixture();
    let change_set = clear_glyphs(
        &mut f.doc,
        &ClearGlyphs {
            glyph_set_id: f.glyph_set,
            pages: PageSelector::Id(f.bold),
            glyphs: GlyphSelector::All,
        },
    )
    .unwrap();
    assert!(change_set.is_empty());
}

#[test]
fn shift_by_zero_is_a_noop() {
    let mut f = fixture();
    let change_set = shift_glyphs(
        &mut f.doc,
        &ShiftGlyphs {
            glyph_set_id: f.glyph_set,
            pages: PageSelector::All,
            glyphs: GlyphSelector::All,
            dx: 0,
            dy: 0,
            overflow: OverflowPolicy::Discard,
        },
    )
    .unwrap();
    assert!(change_set.is_empty());
}

#[test]
fn batch_op_only_touches_existing_glyphs() {
    // Only 0x41 is drawn; selecting All should change exactly one glyph.
    let mut f = fixture();
    let change_set = invert_glyphs(
        &mut f.doc,
        &InvertGlyphs {
            glyph_set_id: f.glyph_set,
            pages: PageSelector::Name("Regular".into()),
            glyphs: GlyphSelector::All,
        },
    )
    .unwrap();
    assert_eq!(change_set.object_changes.len(), 1);
}

#[test]
fn selector_errors_are_precise() {
    let mut f = fixture();
    // Unknown page index.
    let err = shift_glyphs(
        &mut f.doc,
        &ShiftGlyphs {
            glyph_set_id: f.glyph_set,
            pages: PageSelector::Index(9),
            glyphs: GlyphSelector::All,
            dx: 1,
            dy: 0,
            overflow: OverflowPolicy::Discard,
        },
    )
    .unwrap_err();
    assert!(matches!(
        err,
        FontSpaceError::PageIndexOutOfRange {
            index: 9,
            len: 2,
            ..
        }
    ));

    // Code with no entry.
    let err = clear_glyphs(
        &mut f.doc,
        &ClearGlyphs {
            glyph_set_id: f.glyph_set,
            pages: PageSelector::All,
            glyphs: GlyphSelector::Code(0x99),
        },
    )
    .unwrap_err();
    assert!(matches!(
        err,
        FontSpaceError::CodeNotInCharacterSet { code: 0x99, .. }
    ));
}

#[test]
fn batch_op_bad_selector_leaves_document_unchanged() {
    let mut f = fixture();
    let before = f.doc.clone();
    let err = clear_glyphs(
        &mut f.doc,
        &ClearGlyphs {
            glyph_set_id: f.glyph_set,
            pages: PageSelector::Index(99), // resolves to an error before any mutation
            glyphs: GlyphSelector::All,
        },
    )
    .unwrap_err();
    assert!(matches!(
        err,
        FontSpaceError::PageIndexOutOfRange { index: 99, .. }
    ));
    assert_eq!(
        f.doc, before,
        "a failed batch op leaves the document unchanged"
    );
}

#[test]
fn duplicate_codes_selector_is_deduped() {
    let mut f = fixture();
    let change_set = clear_glyphs(
        &mut f.doc,
        &ClearGlyphs {
            glyph_set_id: f.glyph_set,
            pages: PageSelector::Name("Regular".into()),
            glyphs: GlyphSelector::Codes(vec![0x41, 0x41, 0x41]),
        },
    )
    .unwrap();
    assert_eq!(
        change_set.object_changes.len(),
        1,
        "duplicate codes collapse to one change"
    );
}

#[test]
fn ambiguous_page_name_is_rejected() {
    let mut f = fixture();
    // Give the Bold page the same name as Regular.
    f.doc.glyph_sets[0].pages[1].name = "Regular".into();
    let err = clear_glyphs(
        &mut f.doc,
        &ClearGlyphs {
            glyph_set_id: f.glyph_set,
            pages: PageSelector::Name("Regular".into()),
            glyphs: GlyphSelector::All,
        },
    )
    .unwrap_err();
    assert!(matches!(
        err,
        FontSpaceError::AmbiguousPageName { count: 2, .. }
    ));
}

#[test]
fn code_range_selects_entries_within_range() {
    // Draw glyphs for 0x41, 0x42, 0x43, then clear only 0x42..=0x43.
    let mut f = fixture();
    let size = f.doc.glyph_sets[0].glyph_size;
    for code in [0x42u32, 0x43] {
        let mut b = Bitmap::new_blank(size);
        b.set(0, 0, true).unwrap();
        f.doc.glyph_sets[0].pages[0]
            .glyphs
            .push(Glyph { code, bitmap: b });
    }
    let change_set = clear_glyphs(
        &mut f.doc,
        &ClearGlyphs {
            glyph_set_id: f.glyph_set,
            pages: PageSelector::Name("Regular".into()),
            glyphs: GlyphSelector::CodeRangeInclusive {
                start: 0x42,
                end: 0x43,
            },
        },
    )
    .unwrap();
    assert_eq!(change_set.object_changes.len(), 2); // 0x42 and 0x43, not 0x41
    assert!(
        f.doc.glyph_sets[0].pages[0]
            .glyph_of_code(0x41)
            .unwrap()
            .bitmap
            .get(1, 1)
            .unwrap()
    );
}

// --- Page operations ---

#[test]
fn add_page_appends_and_undo_removes() {
    let mut f = fixture();
    let before = f.doc.clone();
    let change_set = add_page(
        &mut f.doc,
        &AddPage {
            glyph_set_id: f.glyph_set,
            name: "Italic".into(),
            description: String::new(),
            at_index: None,
        },
        &mut f.ids,
    )
    .unwrap();
    assert_eq!(f.doc.glyph_sets[0].pages.len(), 3);
    assert_eq!(f.doc.glyph_sets[0].pages[2].name, "Italic");
    undo(&mut f.doc, &change_set).unwrap();
    assert_eq!(f.doc, before, "undo of add_page removes the page");
}

#[test]
fn add_page_at_index_inserts_there() {
    let mut f = fixture();
    add_page(
        &mut f.doc,
        &AddPage {
            glyph_set_id: f.glyph_set,
            name: "First".into(),
            description: String::new(),
            at_index: Some(0),
        },
        &mut f.ids,
    )
    .unwrap();
    assert_eq!(f.doc.glyph_sets[0].pages[0].name, "First");
    assert_eq!(f.doc.glyph_sets[0].pages[1].name, "Regular");
}

#[test]
fn remove_pages_and_undo_restores_positions() {
    let mut f = fixture();
    let before = f.doc.clone();
    // Remove the Regular page (index 0), keeping Bold.
    let change_set = remove_pages(
        &mut f.doc,
        &RemovePages {
            glyph_set_id: f.glyph_set,
            pages: PageSelector::Name("Regular".into()),
        },
    )
    .unwrap();
    assert_eq!(f.doc.glyph_sets[0].pages.len(), 1);
    assert_eq!(f.doc.glyph_sets[0].pages[0].name, "Bold");
    undo(&mut f.doc, &change_set).unwrap();
    assert_eq!(
        f.doc, before,
        "undo restores the page at its original index, with glyphs"
    );
}

#[test]
fn reorder_pages_and_undo() {
    let mut f = fixture();
    let before = f.doc.clone();
    let change_set = reorder_pages(
        &mut f.doc,
        &ReorderPages {
            glyph_set_id: f.glyph_set,
            order: vec![f.bold, f.regular],
        },
    )
    .unwrap();
    assert_eq!(f.doc.glyph_sets[0].pages[0].id, f.bold);
    assert_eq!(f.doc.glyph_sets[0].pages[1].id, f.regular);
    undo(&mut f.doc, &change_set).unwrap();
    assert_eq!(f.doc, before);
}

#[test]
fn reorder_rejects_non_permutation() {
    let mut f = fixture();
    let err = reorder_pages(
        &mut f.doc,
        &ReorderPages {
            glyph_set_id: f.glyph_set,
            order: vec![f.regular], // missing bold
        },
    )
    .unwrap_err();
    assert!(matches!(
        err,
        FontSpaceError::InvalidPageOrder { expected: 2, .. }
    ));
}

#[test]
fn reorder_to_same_order_is_a_noop() {
    let mut f = fixture();
    let change_set = reorder_pages(
        &mut f.doc,
        &ReorderPages {
            glyph_set_id: f.glyph_set,
            order: vec![f.regular, f.bold],
        },
    )
    .unwrap();
    assert!(change_set.is_empty());
}

// --- Guide operations ---

#[test]
fn add_guide_and_undo() {
    let mut f = fixture();
    let before = f.doc.clone();
    let change_set = add_guide(
        &mut f.doc,
        &AddGuide {
            glyph_set_id: f.glyph_set,
            page_id: f.regular,
            name: "Baseline".into(),
            axis: GuideAxis::Horizontal,
            position: 6,
            visible: true,
            locked: false,
        },
        &mut f.ids,
    )
    .unwrap();
    assert_eq!(f.doc.glyph_sets[0].pages[0].guides.len(), 1);
    undo(&mut f.doc, &change_set).unwrap();
    assert_eq!(f.doc, before, "undo of add_guide removes the guide");
}

#[test]
fn move_guide_and_undo() {
    let mut f = fixture();
    add_guide(
        &mut f.doc,
        &AddGuide {
            glyph_set_id: f.glyph_set,
            page_id: f.regular,
            name: "Baseline".into(),
            axis: GuideAxis::Horizontal,
            position: 6,
            visible: true,
            locked: false,
        },
        &mut f.ids,
    )
    .unwrap();
    let guide_id = f.doc.glyph_sets[0].pages[0].guides[0].id;
    let after_add = f.doc.clone();

    let change_set = move_guide(
        &mut f.doc,
        &MoveGuide {
            glyph_set_id: f.glyph_set,
            page_id: f.regular,
            guide_id,
            position: 10,
        },
    )
    .unwrap();
    assert_eq!(f.doc.glyph_sets[0].pages[0].guides[0].position, 10);
    undo(&mut f.doc, &change_set).unwrap();
    assert_eq!(f.doc, after_add, "undo restores the guide position");
}

#[test]
fn move_guide_to_same_position_is_a_noop() {
    let mut f = fixture();
    add_guide(
        &mut f.doc,
        &AddGuide {
            glyph_set_id: f.glyph_set,
            page_id: f.regular,
            name: "Baseline".into(),
            axis: GuideAxis::Horizontal,
            position: 6,
            visible: true,
            locked: false,
        },
        &mut f.ids,
    )
    .unwrap();
    let guide_id = f.doc.glyph_sets[0].pages[0].guides[0].id;
    let change_set = move_guide(
        &mut f.doc,
        &MoveGuide {
            glyph_set_id: f.glyph_set,
            page_id: f.regular,
            guide_id,
            position: 6,
        },
    )
    .unwrap();
    assert!(change_set.is_empty());
}

#[test]
fn move_missing_guide_errors() {
    let mut f = fixture();
    let mut ids = SequentialIdGen::new();
    let bogus = fontspace_model::GuideId::new(&mut ids);
    let err = move_guide(
        &mut f.doc,
        &MoveGuide {
            glyph_set_id: f.glyph_set,
            page_id: f.regular,
            guide_id: bogus,
            position: 3,
        },
    )
    .unwrap_err();
    assert!(matches!(err, FontSpaceError::GuideNotFound { .. }));
}

#[test]
fn copy_guide_to_pages_mints_fresh_ids_and_skips_source() {
    let mut f = fixture();
    add_guide(
        &mut f.doc,
        &AddGuide {
            glyph_set_id: f.glyph_set,
            page_id: f.regular,
            name: "Baseline".into(),
            axis: GuideAxis::Horizontal,
            position: 6,
            visible: true,
            locked: false,
        },
        &mut f.ids,
    )
    .unwrap();
    let source_guide_id = f.doc.glyph_sets[0].pages[0].guides[0].id;
    let before = f.doc.clone();

    let change_set = copy_guide_to_pages(
        &mut f.doc,
        &CopyGuideToPages {
            glyph_set_id: f.glyph_set,
            source_page_id: f.regular,
            guide_id: source_guide_id,
            target_pages: PageSelector::All, // includes source; source is skipped
        },
        &mut f.ids,
    )
    .unwrap();
    // Bold page (index 1) got a copy with a distinct id; source unchanged (1 guide).
    assert_eq!(f.doc.glyph_sets[0].pages[0].guides.len(), 1);
    assert_eq!(f.doc.glyph_sets[0].pages[1].guides.len(), 1);
    let copied = &f.doc.glyph_sets[0].pages[1].guides[0];
    assert_ne!(copied.id, source_guide_id, "copy mints a fresh GuideId");
    assert_eq!(copied.position, 6);
    undo(&mut f.doc, &change_set).unwrap();
    assert_eq!(f.doc, before, "undo removes every copied guide");
}

// --- Property test: every operation's change set inverts exactly (spec/07 §7.7) ---

prop_compose! {
    fn arb_edits()(edits in prop::collection::vec(
        (0u16..8, 0u16..8, any::<bool>()), 0..12,
    )) -> Vec<PixelEdit> {
        edits.into_iter().map(|(x, y, value)| PixelEdit { x, y, value }).collect()
    }
}

proptest! {
    #[test]
    fn set_pixels_then_undo_restores(edits in arb_edits()) {
        let mut f = fixture();
        let original = pruned(&f.doc);
        let req = SetPixels {
            target: GlyphRef { glyph_set_id: f.glyph_set, page_id: f.regular, code: 0x41 },
            edits,
        };
        let change_set = set_pixels(&mut f.doc, &req).unwrap();
        undo(&mut f.doc, &change_set).unwrap();
        prop_assert_eq!(pruned(&f.doc), original);
    }

    #[test]
    fn shift_then_undo_restores(dx in -10i16..=10, dy in -10i16..=10, wrap in any::<bool>()) {
        let mut f = fixture();
        let original = pruned(&f.doc);
        let overflow = if wrap { OverflowPolicy::Wrap } else { OverflowPolicy::Discard };
        let change_set = shift_glyphs(&mut f.doc, &ShiftGlyphs {
            glyph_set_id: f.glyph_set,
            pages: PageSelector::All,
            glyphs: GlyphSelector::All,
            dx, dy, overflow,
        }).unwrap();
        undo(&mut f.doc, &change_set).unwrap();
        prop_assert_eq!(pruned(&f.doc), original);
    }
}
