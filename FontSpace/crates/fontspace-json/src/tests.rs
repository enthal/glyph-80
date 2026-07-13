//! Round-trip, canonical-writer, golden, and property tests for JSON persistence
//! (spec/06, spec/15 §15.2–15.4). All ids come from `SequentialIdGen`, so documents
//! and their JSON are byte-for-byte reproducible.

use fontspace_model::{
    Bitmap, CharacterEntry, CharacterSet, FontSpace, Glyph, GlyphPage, GlyphSet, GlyphSize, Guide,
    GuideAxis, GuideId, SequentialIdGen, ValidationWarning,
};
use proptest::prelude::*;
use serde_json::json;

use crate::{JsonError, load, save};

/// A representative document exercising: charset-entry ordering (glyphs stored out
/// of order), blank-glyph pruning (a blank 0x20), a guide, and 8×8 visual rows.
fn golden_doc() -> FontSpace {
    let mut ids = SequentialIdGen::new();
    let mut character_set = CharacterSet::new(&mut ids, "ASCII subset", "space and two letters");
    character_set.entries = vec![
        CharacterEntry {
            code: 0x20,
            label: "SPACE".into(),
        },
        CharacterEntry {
            code: 0x41,
            label: "LATIN CAPITAL LETTER A".into(),
        },
        CharacterEntry {
            code: 0x42,
            label: "LATIN CAPITAL LETTER B".into(),
        },
    ];

    let size = GlyphSize::new(8, 8);
    let mut glyph_set = GlyphSet::new(&mut ids, "Terminal 8x8", "demo", size, character_set.id);
    let mut page = GlyphPage::new(&mut ids, "Regular", "primary page");
    page.guides.push(Guide {
        id: GuideId::new(&mut ids),
        name: "Baseline".into(),
        axis: GuideAxis::Horizontal,
        position: 7,
        visible: true,
        locked: false,
    });

    let mut letter_a = Bitmap::new_blank(size);
    for (x, y) in [
        (3, 1),
        (4, 1),
        (2, 2),
        (5, 2),
        (2, 3),
        (3, 3),
        (4, 3),
        (5, 3),
        (2, 4),
        (5, 4),
    ] {
        letter_a.set(x, y, true).unwrap();
    }
    let mut letter_b = Bitmap::new_blank(size);
    for (x, y) in [
        (2, 1),
        (3, 1),
        (2, 2),
        (4, 2),
        (2, 3),
        (3, 3),
        (2, 4),
        (4, 4),
        (2, 5),
    ] {
        letter_b.set(x, y, true).unwrap();
    }

    // Push out of entry order (B then A) and include a blank 0x20 to prove that save
    // reorders to entry order and prunes blanks.
    page.glyphs.push(Glyph {
        code: 0x42,
        bitmap: letter_b,
    });
    page.glyphs.push(Glyph {
        code: 0x41,
        bitmap: letter_a,
    });
    page.glyphs.push(Glyph {
        code: 0x20,
        bitmap: Bitmap::new_blank(size),
    });
    glyph_set.pages.push(page);

    let mut doc = FontSpace::new(&mut ids, "Golden", "canonical golden document");
    doc.character_sets.push(character_set);
    doc.glyph_sets.push(glyph_set);
    doc
}

#[test]
fn golden_basic_document_matches_file() {
    let json = save(&golden_doc());
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/testdata/basic.fontspace.json");
    if std::env::var("UPDATE_GOLDEN").is_ok() {
        std::fs::write(path, &json).expect("write golden");
    }
    let expected =
        std::fs::read_to_string(path).expect("golden file missing; run with UPDATE_GOLDEN=1");
    assert_eq!(
        json, expected,
        "canonical output drifted from the golden file"
    );
}

#[test]
fn save_is_canonical_shape() {
    let json = save(&golden_doc());
    assert!(json.ends_with("\n"), "exactly one trailing newline");
    assert!(!json.ends_with("\n\n"));
    assert!(json.contains("\n  \"format_version\""), "2-space indent");
    // code written as lowercase 0x hex; pixels as . / #
    assert!(json.contains("\"0x41\""));
    assert!(json.contains("\"axis\": \"horizontal\""));
}

#[test]
fn glyphs_saved_in_entry_order_with_blanks_pruned() {
    let outcome = load(&save(&golden_doc())).unwrap();
    let page = &outcome.document.glyph_sets[0].pages[0];
    // Blank 0x20 pruned; A and B present in entry order (0x41 before 0x42), despite
    // being inserted B-then-A.
    let codes: Vec<u32> = page.glyphs.iter().map(|g| g.code).collect();
    assert_eq!(codes, vec![0x41, 0x42]);
    assert!(page.glyph_of_code(0x20).is_none());
}

#[test]
fn save_load_save_is_byte_identical() {
    let once = save(&golden_doc());
    let reloaded = load(&once).unwrap().document;
    let twice = save(&reloaded);
    assert_eq!(once, twice, "save(load(save(d))) must equal save(d)");
}

#[test]
fn load_reports_dangling_glyph_as_warning_not_error() {
    let mut doc = golden_doc();
    let size = doc.glyph_sets[0].glyph_size;
    let mut mark = Bitmap::new_blank(size);
    mark.set(0, 0, true).unwrap(); // non-blank so it survives pruning
    doc.glyph_sets[0].pages[0].glyphs.push(Glyph {
        code: 0x99, // no entry for this code
        bitmap: mark,
    });
    let outcome = load(&save(&doc)).expect("dangling glyph must not fail the load");
    assert_eq!(outcome.warnings.len(), 1);
    assert!(matches!(
        outcome.warnings[0],
        ValidationWarning::DanglingGlyph { code: 0x99, .. }
    ));
}

#[test]
fn unsupported_version_is_rejected() {
    let json = json!({
        "format_version": 999,
        "id": "00000000-0000-0000-0000-000000000001",
        "metadata": { "name": "x", "description": "" }
    })
    .to_string();
    let err = load(&json).unwrap_err();
    assert!(matches!(
        err,
        JsonError::UnsupportedVersion { found: 999, .. }
    ));
}

#[test]
fn malformed_visual_row_is_a_precise_error() {
    // A 2×2 glyph set, but the glyph gives a 3-wide row.
    let json = json!({
        "format_version": 1,
        "id": "00000000-0000-0000-0000-000000000001",
        "metadata": { "name": "x", "description": "" },
        "character_sets": [
            { "id": "00000000-0000-0000-0000-000000000002", "name": "cs", "description": "",
              "entries": [ { "code": "0x41", "label": "A" } ] }
        ],
        "glyph_sets": [
            { "id": "00000000-0000-0000-0000-000000000003", "name": "gs", "description": "",
              "glyph_size": { "width": 2, "height": 2 },
              "character_set_id": "00000000-0000-0000-0000-000000000002",
              "pages": [
                { "id": "00000000-0000-0000-0000-000000000004", "name": "p", "description": "",
                  "glyphs": [ { "code": "0x41", "pixels": ["###", ".."] } ] }
              ] }
        ]
    })
    .to_string();
    match load(&json).unwrap_err() {
        JsonError::RowWidth {
            row,
            expected,
            found,
            ..
        } => assert_eq!((row, expected, found), (0, 2, 3)),
        other => panic!("expected RowWidth, got {other:?}"),
    }
}

#[test]
fn structurally_invalid_document_fails_load() {
    // Two glyphs for the same code on one page -> hard validation error.
    let json = json!({
        "format_version": 1,
        "id": "00000000-0000-0000-0000-000000000001",
        "metadata": { "name": "x", "description": "" },
        "character_sets": [
            { "id": "00000000-0000-0000-0000-000000000002", "name": "cs", "description": "",
              "entries": [ { "code": "0x41", "label": "A" } ] }
        ],
        "glyph_sets": [
            { "id": "00000000-0000-0000-0000-000000000003", "name": "gs", "description": "",
              "glyph_size": { "width": 1, "height": 1 },
              "character_set_id": "00000000-0000-0000-0000-000000000002",
              "pages": [
                { "id": "00000000-0000-0000-0000-000000000004", "name": "p", "description": "",
                  "glyphs": [ { "code": "0x41", "pixels": ["#"] }, { "code": "0x41", "pixels": ["#"] } ] }
              ] }
        ]
    })
    .to_string();
    assert!(matches!(load(&json).unwrap_err(), JsonError::Invalid(_)));
}

#[test]
fn invalid_uuid_is_a_precise_error() {
    let json = json!({
        "format_version": 1,
        "id": "not-a-uuid",
        "metadata": { "name": "x", "description": "" }
    })
    .to_string();
    assert!(matches!(
        load(&json).unwrap_err(),
        JsonError::InvalidUuid { .. }
    ));
}

// --- Property test: JSON round-trip preserves valid documents (spec/15 §15.4) ---

prop_compose! {
    /// A single-page 8×8 glyph set over codes {0x41, 0x42}, each glyph a random
    /// pattern. Content varies; ids are fixed (deterministic).
    fn arb_doc()(
        a_bits in prop::collection::vec(any::<bool>(), 64),
        b_bits in prop::collection::vec(any::<bool>(), 64),
    ) -> FontSpace {
        let mut ids = SequentialIdGen::new();
        let mut cs = CharacterSet::new(&mut ids, "cs", "");
        cs.entries = vec![
            CharacterEntry { code: 0x41, label: "A".into() },
            CharacterEntry { code: 0x42, label: "B".into() },
        ];
        let size = GlyphSize::new(8, 8);
        let mut gs = GlyphSet::new(&mut ids, "gs", "", size, cs.id);
        let mut page = GlyphPage::new(&mut ids, "p", "");
        for (code, bits) in [(0x41u32, &a_bits), (0x42u32, &b_bits)] {
            let mut bitmap = Bitmap::new_blank(size);
            for (i, &on) in bits.iter().enumerate() {
                if on {
                    bitmap.set((i % 8) as u16, (i / 8) as u16, true).unwrap();
                }
            }
            page.glyphs.push(Glyph { code, bitmap });
        }
        gs.pages.push(page);
        let mut doc = FontSpace::new(&mut ids, "doc", "");
        doc.character_sets.push(cs);
        doc.glyph_sets.push(gs);
        doc
    }
}

proptest! {
    #[test]
    fn round_trip_is_byte_stable(doc in arb_doc()) {
        let once = save(&doc);
        let reloaded = load(&once).unwrap().document;
        prop_assert_eq!(&save(&reloaded), &once);
    }

    #[test]
    fn round_trip_preserves_non_blank_glyphs(doc in arb_doc()) {
        // Every non-blank glyph survives load with identical pixels.
        let reloaded = load(&save(&doc)).unwrap().document;
        let src_page = &doc.glyph_sets[0].pages[0];
        let dst_page = &reloaded.glyph_sets[0].pages[0];
        for glyph in &src_page.glyphs {
            if glyph.bitmap.is_blank() {
                continue;
            }
            let loaded = dst_page.glyph_of_code(glyph.code).expect("non-blank glyph preserved");
            prop_assert_eq!(&loaded.bitmap, &glyph.bitmap);
        }
    }
}
