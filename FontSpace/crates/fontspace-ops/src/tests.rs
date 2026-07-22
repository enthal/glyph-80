//! Operation tests (spec/07, spec/15 §15.1/§15.4). Strict-layer focus: atomicity
//! and exact change-set inversion (undo). All ids from `SequentialIdGen`.

use fontspace_model::{
    Bitmap, CharacterEntry, CharacterSet, CharacterSetId, FontSpace, Glyph, GlyphPage, GlyphSet,
    GlyphSetId, GlyphSize, Guide, GuideAxis, GuideId, Limits, OverflowPolicy, PageId,
    SequentialIdGen,
};
use proptest::prelude::*;

use crate::apply_change_set;
use crate::change_set::{ChangeSet, GlyphSetChange, ObjectChange};
use crate::{
    AddCharacterEntry, AddExportConfig, AddGlyphSet, AddGuide, AddPage, ClearGlyphs,
    CopyGuideToPages, DuplicateGlyphSet, FontSpaceError, FontSpaceWarning, GlyphRef, GlyphSelector,
    InvertGlyphs, MoveGuide, PageSelector, PixelEdit, RecodeCharacterEntry, RemoveCharacterEntry,
    RemoveGlyphSet, RemoveGuide, RemovePages, RenameGlyphSet, RenameGuide, ReorderCharacterEntries,
    ReorderPages, ReplaceExportConfig, SetGuideVisible, SetPixels, ShiftGlyphs,
    add_character_entry, add_export_config, add_glyph_set, add_guide, add_page, clear_glyphs,
    copy_guide_to_pages, duplicate_glyph_set, invert_glyphs, move_guide, recode_character_entry,
    remove_character_entry, remove_glyph_set, remove_guide, remove_pages, rename_glyph_set,
    rename_guide, reorder_character_entries, reorder_pages, replace_export_config,
    set_guide_visible, set_pixels, shift_glyphs, undo,
};

struct Fixture {
    doc: FontSpace,
    character_set: CharacterSetId,
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

    let character_set_id = character_set.id;
    let mut doc = FontSpace::new(&mut ids, "doc", "");
    doc.character_sets.push(character_set);
    doc.glyph_sets.push(glyph_set);
    Fixture {
        doc,
        character_set: character_set_id,
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
fn remove_guide_and_undo() {
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

    let change_set = remove_guide(
        &mut f.doc,
        &RemoveGuide {
            glyph_set_id: f.glyph_set,
            page_id: f.regular,
            guide_id,
        },
    )
    .unwrap();
    assert!(f.doc.glyph_sets[0].pages[0].guides.is_empty());
    undo(&mut f.doc, &change_set).unwrap();
    assert_eq!(f.doc, after_add, "undo of remove_guide restores the guide");
}

#[test]
fn remove_non_last_guide_undo_restores_order() {
    // Exact inversion (spec/07 §7.7): removing a non-last guide and undoing must
    // restore it at its original index, not append it.
    let mut f = fixture();
    let add = |f: &mut Fixture, name: &str, pos: i32| {
        add_guide(
            &mut f.doc,
            &AddGuide {
                glyph_set_id: f.glyph_set,
                page_id: f.regular,
                name: name.into(),
                axis: GuideAxis::Horizontal,
                position: pos,
                visible: true,
                locked: false,
            },
            &mut f.ids,
        )
        .unwrap();
    };
    add(&mut f, "A", 1);
    add(&mut f, "B", 2);
    let after_adds = f.doc.clone();
    let first = f.doc.glyph_sets[0].pages[0].guides[0].id; // "A", index 0

    let change_set = remove_guide(
        &mut f.doc,
        &RemoveGuide {
            glyph_set_id: f.glyph_set,
            page_id: f.regular,
            guide_id: first,
        },
    )
    .unwrap();
    // Only "B" remains.
    let names: Vec<_> = f.doc.glyph_sets[0].pages[0]
        .guides
        .iter()
        .map(|g| g.name.as_str())
        .collect();
    assert_eq!(names, ["B"]);

    undo(&mut f.doc, &change_set).unwrap();
    assert_eq!(
        f.doc, after_adds,
        "undo restores the removed guide at its original index"
    );
}

#[test]
fn set_guide_visible_and_undo() {
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

    let change_set = set_guide_visible(
        &mut f.doc,
        &SetGuideVisible {
            glyph_set_id: f.glyph_set,
            page_id: f.regular,
            guide_id,
            visible: false,
        },
    )
    .unwrap();
    assert!(!f.doc.glyph_sets[0].pages[0].guides[0].visible);
    undo(&mut f.doc, &change_set).unwrap();
    assert_eq!(f.doc, after_add, "undo restores visibility");

    // Setting to the current value is a no-op change set.
    let noop = set_guide_visible(
        &mut f.doc,
        &SetGuideVisible {
            glyph_set_id: f.glyph_set,
            page_id: f.regular,
            guide_id,
            visible: true,
        },
    )
    .unwrap();
    assert!(noop.is_empty());
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
fn rename_guide_and_undo() {
    let mut f = fixture();
    add_guide(
        &mut f.doc,
        &AddGuide {
            glyph_set_id: f.glyph_set,
            page_id: f.regular,
            name: "h-guide".into(),
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

    let change_set = rename_guide(
        &mut f.doc,
        &RenameGuide {
            glyph_set_id: f.glyph_set,
            page_id: f.regular,
            guide_id,
            name: "Baseline".into(),
        },
    )
    .unwrap();
    assert_eq!(f.doc.glyph_sets[0].pages[0].guides[0].name, "Baseline");
    undo(&mut f.doc, &change_set).unwrap();
    assert_eq!(f.doc, after_add, "undo restores the guide name");

    // Renaming to the current name is a no-op change set.
    let noop = rename_guide(
        &mut f.doc,
        &RenameGuide {
            glyph_set_id: f.glyph_set,
            page_id: f.regular,
            guide_id,
            name: "h-guide".into(),
        },
    )
    .unwrap();
    assert!(noop.is_empty());
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

// --- Character-set entry operations (add / reorder / recode) ---

#[test]
fn add_character_entry_and_undo() {
    let mut f = fixture();
    let before = f.doc.clone();
    let change_set = add_character_entry(
        &mut f.doc,
        &AddCharacterEntry {
            character_set_id: f.character_set,
            code: 0x44,
            label: "D".into(),
            at_index: None,
        },
    )
    .unwrap();
    assert!(f.doc.character_sets[0].contains_code(0x44));
    undo(&mut f.doc, &change_set).unwrap();
    assert_eq!(
        f.doc, before,
        "undo of add entry removes it; no glyph changes"
    );
}

#[test]
fn add_duplicate_entry_code_errors() {
    let mut f = fixture();
    let err = add_character_entry(
        &mut f.doc,
        &AddCharacterEntry {
            character_set_id: f.character_set,
            code: 0x41, // already present
            label: "A2".into(),
            at_index: None,
        },
    )
    .unwrap_err();
    assert!(matches!(
        err,
        FontSpaceError::DuplicateEntryCode { code: 0x41, .. }
    ));
}

#[test]
fn reorder_entries_touches_no_glyphs_and_undoes() {
    let mut f = fixture();
    let before = f.doc.clone();
    let glyph_before = f.doc.glyph_sets[0].pages[0]
        .glyph_of_code(0x41)
        .unwrap()
        .bitmap
        .clone();
    let change_set = reorder_character_entries(
        &mut f.doc,
        &ReorderCharacterEntries {
            character_set_id: f.character_set,
            order: vec![0x43, 0x42, 0x41],
        },
    )
    .unwrap();
    // Entry order changed...
    let codes: Vec<u32> = f.doc.character_sets[0]
        .entries
        .iter()
        .map(|e| e.code)
        .collect();
    assert_eq!(codes, vec![0x43, 0x42, 0x41]);
    // ...but the glyph is untouched (code identity, not ordinal).
    assert_eq!(
        f.doc.glyph_sets[0].pages[0]
            .glyph_of_code(0x41)
            .unwrap()
            .bitmap,
        glyph_before
    );
    undo(&mut f.doc, &change_set).unwrap();
    assert_eq!(f.doc, before);
}

#[test]
fn reorder_entries_rejects_non_permutation() {
    let mut f = fixture();
    let err = reorder_character_entries(
        &mut f.doc,
        &ReorderCharacterEntries {
            character_set_id: f.character_set,
            order: vec![0x41, 0x42], // missing 0x43
        },
    )
    .unwrap_err();
    assert!(matches!(
        err,
        FontSpaceError::InvalidEntryOrder { expected: 3, .. }
    ));
}

#[test]
fn recode_entry_warns_about_orphaned_glyph_and_moves_nothing() {
    let mut f = fixture();
    let glyph_before = f.doc.glyph_sets[0].pages[0]
        .glyph_of_code(0x41)
        .unwrap()
        .bitmap
        .clone();
    let before = f.doc.clone();
    // 0x41 has a drawn glyph; recoding the entry 0x41 -> 0x50 orphans it.
    let change_set = recode_character_entry(
        &mut f.doc,
        &RecodeCharacterEntry {
            character_set_id: f.character_set,
            from_code: 0x41,
            to_code: 0x50,
        },
    )
    .unwrap();
    // Entry recoded.
    assert!(f.doc.character_sets[0].contains_code(0x50));
    assert!(!f.doc.character_sets[0].contains_code(0x41));
    // Glyph data untouched — the 0x41 glyph is still there (now dangling), unchanged.
    assert_eq!(
        f.doc.glyph_sets[0].pages[0]
            .glyph_of_code(0x41)
            .unwrap()
            .bitmap,
        glyph_before
    );
    // Warned about the orphan.
    assert_eq!(change_set.warnings.len(), 1);
    let FontSpaceWarning::RecodeOrphanedGlyphs {
        orphaned,
        from_code: 0x41,
        to_code: 0x50,
        ..
    } = &change_set.warnings[0]
    else {
        panic!("expected RecodeOrphanedGlyphs");
    };
    assert_eq!(orphaned.len(), 1);
    undo(&mut f.doc, &change_set).unwrap();
    assert_eq!(
        f.doc, before,
        "undo restores the entry code; glyph never moved"
    );
}

#[test]
fn recode_finds_orphans_on_every_page() {
    let mut f = fixture();
    // Draw a second 0x41 glyph on the Bold page, so both pages hold one.
    let size = f.doc.glyph_sets[0].glyph_size;
    let mut mark = Bitmap::new_blank(size);
    mark.set(0, 0, true).unwrap();
    f.doc.glyph_sets[0].pages[1].glyphs.push(Glyph {
        code: 0x41,
        bitmap: mark,
    });
    let change_set = recode_character_entry(
        &mut f.doc,
        &RecodeCharacterEntry {
            character_set_id: f.character_set,
            from_code: 0x41,
            to_code: 0x50,
        },
    )
    .unwrap();
    let FontSpaceWarning::RecodeOrphanedGlyphs { orphaned, .. } = &change_set.warnings[0] else {
        panic!("expected RecodeOrphanedGlyphs");
    };
    assert_eq!(
        orphaned.len(),
        2,
        "orphans found on both Regular and Bold pages"
    );
}

#[test]
fn remove_entry_cascade_deletes_glyphs_across_pages_and_undo_restores() {
    let mut f = fixture();
    let size = f.doc.glyph_sets[0].glyph_size;
    // Rebuild the Regular page so 0x41 sits at index 1 — with neighbours 0x42 before
    // and 0x43 after — so exact-index restoration is actually exercised (not index 0).
    f.doc.glyph_sets[0].pages[0].glyphs.clear();
    for (code, x) in [(0x42u32, 0u16), (0x41, 1), (0x43, 2)] {
        let mut b = Bitmap::new_blank(size);
        b.set(x, 0, true).unwrap();
        f.doc.glyph_sets[0].pages[0]
            .glyphs
            .push(Glyph { code, bitmap: b });
    }
    // A second 0x41 on the Bold page, so the cascade must reach both pages.
    let mut bold_a = Bitmap::new_blank(size);
    bold_a.set(7, 7, true).unwrap();
    f.doc.glyph_sets[0].pages[1].glyphs.push(Glyph {
        code: 0x41,
        bitmap: bold_a,
    });
    let before = f.doc.clone();

    let change_set = remove_character_entry(
        &mut f.doc,
        &RemoveCharacterEntry {
            character_set_id: f.character_set,
            code: 0x41,
        },
    )
    .unwrap();

    // Entry gone; both 0x41 glyphs cascade-deleted.
    assert!(!f.doc.character_sets[0].contains_code(0x41));
    assert!(f.doc.glyph_sets[0].pages[0].glyph_of_code(0x41).is_none());
    assert!(f.doc.glyph_sets[0].pages[1].glyph_of_code(0x41).is_none());
    // Warning lists both removed glyphs.
    let FontSpaceWarning::RemoveCascade {
        removed,
        code: 0x41,
        ..
    } = &change_set.warnings[0]
    else {
        panic!("expected RemoveCascade");
    };
    assert_eq!(removed.len(), 2);

    // Undo restores the entry and every glyph, exactly (index-preserving).
    undo(&mut f.doc, &change_set).unwrap();
    assert_eq!(
        f.doc, before,
        "cascade undo restores entry and glyphs exactly"
    );
}

#[test]
fn remove_entry_with_no_glyphs_has_no_cascade_warning() {
    let mut f = fixture();
    // 0x43 has an entry but no drawn glyph anywhere.
    let change_set = remove_character_entry(
        &mut f.doc,
        &RemoveCharacterEntry {
            character_set_id: f.character_set,
            code: 0x43,
        },
    )
    .unwrap();
    assert!(!f.doc.character_sets[0].contains_code(0x43));
    assert!(
        change_set.warnings.is_empty(),
        "no glyphs removed => no cascade warning"
    );
    assert_eq!(change_set.object_changes.len(), 1); // just the entry removal
}

#[test]
fn remove_missing_entry_errors() {
    let mut f = fixture();
    let err = remove_character_entry(
        &mut f.doc,
        &RemoveCharacterEntry {
            character_set_id: f.character_set,
            code: 0x99,
        },
    )
    .unwrap_err();
    assert!(matches!(
        err,
        FontSpaceError::EntryCodeNotFound { code: 0x99, .. }
    ));
}

#[test]
fn recode_to_same_code_is_a_noop() {
    let mut f = fixture();
    let change_set = recode_character_entry(
        &mut f.doc,
        &RecodeCharacterEntry {
            character_set_id: f.character_set,
            from_code: 0x41,
            to_code: 0x41,
        },
    )
    .unwrap();
    assert!(change_set.is_empty());
}

#[test]
fn recode_to_existing_code_errors() {
    let mut f = fixture();
    let err = recode_character_entry(
        &mut f.doc,
        &RecodeCharacterEntry {
            character_set_id: f.character_set,
            from_code: 0x41,
            to_code: 0x42, // already an entry
        },
    )
    .unwrap_err();
    assert!(matches!(
        err,
        FontSpaceError::DuplicateEntryCode { code: 0x42, .. }
    ));
}

#[test]
fn recode_from_missing_code_errors() {
    let mut f = fixture();
    let err = recode_character_entry(
        &mut f.doc,
        &RecodeCharacterEntry {
            character_set_id: f.character_set,
            from_code: 0x99, // no such entry
            to_code: 0x50,
        },
    )
    .unwrap_err();
    assert!(matches!(
        err,
        FontSpaceError::EntryCodeNotFound { code: 0x99, .. }
    ));
}

// --- Top-level object ops: add glyph set, add/replace export config (spec/07 §7.2) ---

/// A minimal well-formed export config over `glyph_set`, for the object-op tests. Not a
/// scan preset (that lives in `fontspace-export`); just enough shape to insert and
/// round-trip through undo.
fn sample_export_config(
    ids: &mut SequentialIdGen,
    name: &str,
    glyph_set: &GlyphSet,
) -> fontspace_model::ExportConfig {
    use fontspace_model::{
        AddressBitSource, AddressMap, CoordinateExpr, DataMap, ExportComponentId, ExportConfig,
        ExportConfigId, ExportSourceSpec, OutputBitSource, OutputFormatConfig,
    };
    ExportConfig {
        id: ExportConfigId::new(ids),
        name: name.into(),
        description: String::new(),
        source: ExportSourceSpec {
            glyph_set_id: glyph_set.id,
            pages: glyph_set.pages.iter().map(|page| page.id).collect(),
        },
        address_map: AddressMap {
            id: ExportComponentId::new(ids),
            name: "addr".into(),
            address_bits: vec![AddressBitSource::CodeBit(0)],
        },
        data_map: DataMap {
            id: ExportComponentId::new(ids),
            name: "data".into(),
            output_bits: vec![OutputBitSource::Pixel {
                x: CoordinateExpr::Constant(0),
                y: CoordinateExpr::AddressedY,
            }],
        },
        output_format: OutputFormatConfig::RawBinary,
        output_size: None,
        fill_byte: fontspace_model::DEFAULT_FILL_BYTE,
    }
}

#[test]
fn add_glyph_set_appends_with_a_page_and_undo_removes() {
    let mut f = fixture();
    let before = f.doc.glyph_sets.len();
    let change_set = add_glyph_set(
        &mut f.doc,
        &AddGlyphSet {
            name: "Terminal 8x16".into(),
            description: String::new(),
            glyph_size: GlyphSize::new(8, 16),
            character_set_id: f.character_set,
            initial_page_name: Some("Regular".into()),
        },
        &mut f.ids,
    )
    .unwrap();
    assert_eq!(f.doc.glyph_sets.len(), before + 1);
    let added = f.doc.glyph_sets.last().unwrap();
    assert_eq!(added.name, "Terminal 8x16");
    assert_eq!(added.glyph_size, GlyphSize::new(8, 16));
    assert_eq!(added.pages.len(), 1, "created with its initial page");

    undo(&mut f.doc, &change_set).unwrap();
    assert_eq!(f.doc.glyph_sets.len(), before);
}

#[test]
fn add_glyph_set_rejects_a_missing_character_set() {
    let mut f = fixture();
    let bogus = CharacterSet::new(&mut f.ids, "ghost", "").id;
    let err = add_glyph_set(
        &mut f.doc,
        &AddGlyphSet {
            name: "x".into(),
            description: String::new(),
            glyph_size: GlyphSize::new(8, 8),
            character_set_id: bogus,
            initial_page_name: None,
        },
        &mut f.ids,
    )
    .unwrap_err();
    assert!(matches!(err, FontSpaceError::CharacterSetIdNotFound(id) if id == bogus));
    // Atomic: nothing was added.
    assert_eq!(f.doc.glyph_sets.len(), 1);
}

#[test]
fn add_export_config_appends_and_undo_removes() {
    let mut f = fixture();
    let config = sample_export_config(&mut f.ids, "ROM", &f.doc.glyph_sets[0]);
    let id = config.id;
    let change_set = add_export_config(&mut f.doc, &AddExportConfig { config }).unwrap();
    assert_eq!(f.doc.export_configs.len(), 1);
    assert_eq!(f.doc.export_configs[0].id, id);

    undo(&mut f.doc, &change_set).unwrap();
    assert!(f.doc.export_configs.is_empty());
}

#[test]
fn replace_export_config_swaps_in_place_and_undo_restores() {
    let mut f = fixture();
    let original = sample_export_config(&mut f.ids, "ROM", &f.doc.glyph_sets[0]);
    let id = original.id;
    add_export_config(
        &mut f.doc,
        &AddExportConfig {
            config: original.clone(),
        },
    )
    .unwrap();

    // Edit the name, keeping the same id, and replace.
    let mut edited = original.clone();
    edited.name = "ROM v2".into();
    let change_set =
        replace_export_config(&mut f.doc, &ReplaceExportConfig { config: edited }).unwrap();
    assert_eq!(f.doc.export_configs.len(), 1, "replace, not append");
    assert_eq!(f.doc.export_configs[0].name, "ROM v2");

    undo(&mut f.doc, &change_set).unwrap();
    assert_eq!(f.doc.export_configs[0].id, id);
    assert_eq!(f.doc.export_configs[0].name, "ROM");
}

#[test]
fn rename_glyph_set_changes_the_name_and_undo_restores() {
    let mut f = fixture();
    let change_set = rename_glyph_set(
        &mut f.doc,
        &RenameGlyphSet {
            glyph_set_id: f.glyph_set,
            name: "Renamed".into(),
        },
    )
    .unwrap();
    assert_eq!(f.doc.glyph_sets[0].name, "Renamed");
    undo(&mut f.doc, &change_set).unwrap();
    assert_eq!(f.doc.glyph_sets[0].name, "gs");
}

#[test]
fn rename_glyph_set_to_the_same_name_is_a_noop() {
    let mut f = fixture();
    let change_set = rename_glyph_set(
        &mut f.doc,
        &RenameGlyphSet {
            glyph_set_id: f.glyph_set,
            name: "gs".into(),
        },
    )
    .unwrap();
    assert!(change_set.is_empty(), "unchanged name records nothing");
}

#[test]
fn remove_glyph_set_and_undo_restores_it_whole() {
    let mut f = fixture();
    let before = f.doc.glyph_sets[0].clone();
    let change_set = remove_glyph_set(
        &mut f.doc,
        &RemoveGlyphSet {
            glyph_set_id: f.glyph_set,
        },
    )
    .unwrap();
    assert!(f.doc.glyph_sets.is_empty());
    undo(&mut f.doc, &change_set).unwrap();
    assert_eq!(f.doc.glyph_sets.len(), 1);
    assert_eq!(
        f.doc.glyph_sets[0], before,
        "restored with its pages and glyphs"
    );
}

#[test]
fn duplicate_glyph_set_mints_fresh_ids_after_the_original() {
    let mut f = fixture();
    // Give the Regular page a guide so the copy's guide re-minting is exercised, and the
    // duplicated document must still validate (no duplicate page/guide ids).
    f.doc.glyph_sets[0].pages[0].guides.push(Guide {
        id: GuideId::new(&mut f.ids),
        name: "baseline".into(),
        axis: GuideAxis::Horizontal,
        position: 6,
        visible: true,
        locked: false,
    });

    let change_set = duplicate_glyph_set(
        &mut f.doc,
        &DuplicateGlyphSet {
            glyph_set_id: f.glyph_set,
            name: "Copy".into(),
        },
        &mut f.ids,
    )
    .unwrap();
    assert_eq!(f.doc.glyph_sets.len(), 2);
    let (orig, copy) = (&f.doc.glyph_sets[0], &f.doc.glyph_sets[1]);
    assert_eq!(copy.name, "Copy");
    assert_ne!(copy.id, orig.id, "fresh glyph-set id");
    assert_eq!(copy.glyph_size, orig.glyph_size);
    assert_eq!(
        copy.character_set_id, orig.character_set_id,
        "the copy references the same character set"
    );
    assert_eq!(copy.pages.len(), orig.pages.len());
    // Every page gets a fresh id but copies its glyphs verbatim (glyphs are keyed by code).
    for (orig_page, copy_page) in orig.pages.iter().zip(&copy.pages) {
        assert_ne!(copy_page.id, orig_page.id, "fresh page id");
        assert_eq!(copy_page.glyphs, orig_page.glyphs);
    }
    // The guide is re-minted too — distinct id, same content.
    assert_ne!(copy.pages[0].guides[0].id, orig.pages[0].guides[0].id);
    assert_eq!(copy.pages[0].guides[0].name, orig.pages[0].guides[0].name);
    // No id collides, so the whole document is valid.
    assert!(
        f.doc.validate(&Limits::default()).is_valid(),
        "the duplicated document validates with no duplicate ids"
    );

    undo(&mut f.doc, &change_set).unwrap();
    assert_eq!(f.doc.glyph_sets.len(), 1);
}

#[test]
fn glyph_set_remove_and_reinsert_restores_position() {
    // The strict-layer inversion tests above run on ≤1-element vectors, so they can't
    // catch a re-insert that ignores `index` and appends. Build a 3-set document and
    // remove/undo a *non-tail* set (a change set of the shape `remove_glyph_set` would
    // emit): undo must restore its original position exactly, not push it to the end.
    let mut f = fixture();
    for name in ["second", "third"] {
        add_glyph_set(
            &mut f.doc,
            &AddGlyphSet {
                name: name.into(),
                description: String::new(),
                glyph_size: GlyphSize::new(8, 8),
                character_set_id: f.character_set,
                initial_page_name: None,
            },
            &mut f.ids,
        )
        .unwrap();
    }
    let names_before: Vec<_> = f.doc.glyph_sets.iter().map(|gs| gs.name.clone()).collect();
    assert_eq!(names_before.len(), 3);

    let middle = f.doc.glyph_sets[1].clone();
    let remove = ChangeSet {
        object_changes: vec![ObjectChange::GlyphSetChanged(Box::new(GlyphSetChange {
            index: 1,
            before: Some(middle),
            after: None,
        }))],
        warnings: Vec::new(),
    };
    apply_change_set(&mut f.doc, &remove).unwrap();
    assert_eq!(f.doc.glyph_sets.len(), 2);
    assert_eq!(
        f.doc.glyph_sets[1].name, "third",
        "the middle set was removed"
    );

    undo(&mut f.doc, &remove).unwrap();
    let names_after: Vec<_> = f.doc.glyph_sets.iter().map(|gs| gs.name.clone()).collect();
    assert_eq!(names_after, names_before, "re-insert restores exact order");
}

#[test]
fn replace_export_config_rejects_an_unknown_id() {
    let mut f = fixture();
    // Never added, so its id is not present.
    let orphan = sample_export_config(&mut f.ids, "ghost", &f.doc.glyph_sets[0]);
    let orphan_id = orphan.id;
    let err =
        replace_export_config(&mut f.doc, &ReplaceExportConfig { config: orphan }).unwrap_err();
    assert!(matches!(err, FontSpaceError::ExportConfigNotFound(id) if id == orphan_id));
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
