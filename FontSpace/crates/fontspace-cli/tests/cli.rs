//! End-to-end CLI tests: the same operation invoked via the library and via the CLI
//! binary must produce byte-identical documents (spec/13, spec/15 §15.5).

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use fontspace_model::OverflowPolicy;
use fontspace_model::{
    Bitmap, CharacterEntry, CharacterSet, FontSpace, Glyph, GlyphPage, GlyphSet, GlyphSize,
    SequentialIdGen,
};
use fontspace_ops::{GlyphSelector, PageSelector, ShiftGlyphs, shift_glyphs};

/// A small document: character set (0x41, 0x42), an 8×8 glyph set named "gs" with a
/// "Regular" page holding a drawn glyph for 0x41.
fn sample_doc() -> FontSpace {
    let mut ids = SequentialIdGen::new();
    let mut cs = CharacterSet::new(&mut ids, "cs", "");
    cs.entries = vec![
        CharacterEntry {
            code: 0x41,
            label: "A".into(),
        },
        CharacterEntry {
            code: 0x42,
            label: "B".into(),
        },
    ];
    let size = GlyphSize::new(8, 8);
    let mut gs = GlyphSet::new(&mut ids, "gs", "", size, cs.id);
    let mut page = GlyphPage::new(&mut ids, "Regular", "");
    let mut a = Bitmap::new_blank(size);
    for (x, y) in [(1, 1), (2, 2), (3, 3)] {
        a.set(x, y, true).unwrap();
    }
    page.glyphs.push(Glyph {
        code: 0x41,
        bitmap: a,
    });
    gs.pages.push(page);
    let mut doc = FontSpace::new(&mut ids, "doc", "");
    doc.character_sets.push(cs);
    doc.glyph_sets.push(gs);
    doc
}

fn temp_path(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fontspace-cli-{}-{}", std::process::id(), tag));
    fs::create_dir_all(&dir).unwrap();
    dir.join("t.fontspace.json")
}

#[test]
fn cli_shift_matches_library_shift() {
    let doc = sample_doc();

    // Library: apply the same shift and save canonically.
    let mut via_library = doc.clone();
    let glyph_set_id = via_library.glyph_sets[0].id;
    shift_glyphs(
        &mut via_library,
        &ShiftGlyphs {
            glyph_set_id,
            pages: PageSelector::Name("Regular".into()),
            glyphs: GlyphSelector::Code(0x41),
            dx: 1,
            dy: 0,
            overflow: OverflowPolicy::Discard,
        },
    )
    .unwrap();
    let expected = fontspace_json::save(&via_library);

    // CLI: write the original, run the same shift through the binary.
    let path = temp_path("shift");
    fs::write(&path, fontspace_json::save(&doc)).unwrap();
    let status = Command::new(env!("CARGO_BIN_EXE_fontspace"))
        .args([
            "shift",
            path.to_str().unwrap(),
            "--glyph-set",
            "gs",
            "--pages",
            "Regular",
            "--glyphs",
            "0x41",
            "--dx",
            "1",
            "--dy",
            "0",
        ])
        .status()
        .unwrap();
    assert!(status.success());
    let via_cli = fs::read_to_string(&path).unwrap();

    assert_eq!(
        via_cli, expected,
        "CLI shift must match library shift byte-for-byte"
    );
    fs::remove_dir_all(path.parent().unwrap()).ok();
}

#[test]
fn cli_render_text_matches_render_crate() {
    let doc = sample_doc();
    let path = temp_path("render");
    fs::write(&path, fontspace_json::save(&doc)).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_fontspace"))
        .args([
            "render-text",
            path.to_str().unwrap(),
            "--glyph-set",
            "gs",
            "--glyphs",
            "0x41",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let rendered = String::from_utf8(output.stdout).unwrap();
    // Three on-pixels on the diagonal; a trailing newline from println!.
    let expected =
        "........\n.#......\n..#.....\n...#....\n........\n........\n........\n........\n";
    assert_eq!(rendered, expected);
    fs::remove_dir_all(path.parent().unwrap()).ok();
}

#[test]
fn cli_dry_run_does_not_write() {
    let doc = sample_doc();
    let path = temp_path("dryrun");
    let original = fontspace_json::save(&doc);
    fs::write(&path, &original).unwrap();

    let status = Command::new(env!("CARGO_BIN_EXE_fontspace"))
        .args([
            "shift",
            path.to_str().unwrap(),
            "--glyph-set",
            "gs",
            "--pages",
            "Regular",
            "--glyphs",
            "0x41",
            "--dx",
            "2",
            "--dy",
            "0",
            "--dry-run",
        ])
        .status()
        .unwrap();
    assert!(status.success());
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        original,
        "--dry-run must not write"
    );
    fs::remove_dir_all(path.parent().unwrap()).ok();
}

#[test]
fn cli_unknown_glyph_set_fails_without_writing() {
    let doc = sample_doc();
    let path = temp_path("badgs");
    let original = fontspace_json::save(&doc);
    fs::write(&path, &original).unwrap();

    let status = Command::new(env!("CARGO_BIN_EXE_fontspace"))
        .args([
            "shift",
            path.to_str().unwrap(),
            "--glyph-set",
            "nonesuch",
            "--glyphs",
            "0x41",
            "--dx",
            "1",
            "--dy",
            "0",
        ])
        .status()
        .unwrap();
    assert!(!status.success(), "unknown glyph set must exit non-zero");
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        original,
        "a failed op must not write"
    );
    fs::remove_dir_all(path.parent().unwrap()).ok();
}
