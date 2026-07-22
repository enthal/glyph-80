//! End-to-end CLI tests: the same operation invoked via the library and via the CLI
//! binary must produce byte-identical documents (spec/13, spec/15 §15.5).

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use fontspace_model::OverflowPolicy;
use fontspace_model::{
    Bitmap, CharacterEntry, CharacterSet, FontSpace, FontSpaceFragment, Glyph, GlyphPage, GlyphSet,
    GlyphSize, SequentialIdGen,
};
use fontspace_ops::{
    ExtractGlyphs, GlyphMapping, GlyphSelector, GlyphSizeConversion, PageSelector, PasteGlyphs,
    ShiftGlyphs, extract_glyphs, paste_glyphs, shift_glyphs,
};
use fontspace_render::{
    RenderLayout, TextGridRequest, TextStringRequest, render_text_grid, render_text_string,
};

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
    gs.pages.push(GlyphPage::new(&mut ids, "Bold", "")); // a second, empty page
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

    // Compare against the render crate directly, using the CLI's defaults; the CLI
    // adds one trailing newline via println!.
    let glyph_set_id = doc.glyph_sets[0].id;
    let via_library = render_text_grid(
        &doc,
        &TextGridRequest {
            glyph_set_id,
            pages: PageSelector::All,
            glyphs: GlyphSelector::Code(0x41),
            on: "#".into(),
            off: ".".into(),
            glyph_separator: String::new(),
            row_separator: "\n".into(),
            page_separator: "\n\n".into(),
            layout: RenderLayout::GlyphsHorizontal,
            scale_x: 1,
            scale_y: 1,
        },
    )
    .unwrap();
    assert_eq!(rendered, format!("{via_library}\n"));
    fs::remove_dir_all(path.parent().unwrap()).ok();
}

#[test]
fn cli_render_text_string_matches_render_crate() {
    let doc = sample_doc();
    let path = temp_path("render-text-str");
    fs::write(&path, fontspace_json::save(&doc)).unwrap();

    // `--text AB`: A is drawn, B has an entry but no glyph (renders blank), and the
    // unknown '#' (0x23, no entry) is ignored.
    let output = Command::new(env!("CARGO_BIN_EXE_fontspace"))
        .args([
            "render-text",
            path.to_str().unwrap(),
            "--glyph-set",
            "gs",
            "--text",
            "A#B",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let rendered = String::from_utf8(output.stdout).unwrap();

    let glyph_set_id = doc.glyph_sets[0].id;
    let via_library = render_text_string(
        &doc,
        &TextStringRequest {
            glyph_set_id,
            pages: PageSelector::All,
            rows: vec![vec![0x41, 0x23, 0x42]],
            on: "#".into(),
            off: ".".into(),
            glyph_separator: String::new(),
            row_separator: "\n".into(),
            page_separator: "\n\n".into(),
            scale_x: 1,
            scale_y: 1,
        },
    )
    .unwrap();
    assert_eq!(rendered, format!("{via_library}\n"));
    fs::remove_dir_all(path.parent().unwrap()).ok();
}

#[test]
fn cli_render_text_defaults_to_all_glyphs() {
    let doc = sample_doc();
    let path = temp_path("render-all");
    fs::write(&path, fontspace_json::save(&doc)).unwrap();

    // No render subject → all glyphs.
    let output = Command::new(env!("CARGO_BIN_EXE_fontspace"))
        .args(["render-text", path.to_str().unwrap(), "--glyph-set", "gs"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let rendered = String::from_utf8(output.stdout).unwrap();

    let glyph_set_id = doc.glyph_sets[0].id;
    let via_library = render_text_grid(
        &doc,
        &TextGridRequest {
            glyph_set_id,
            pages: PageSelector::All,
            glyphs: GlyphSelector::All,
            on: "#".into(),
            off: ".".into(),
            glyph_separator: String::new(),
            row_separator: "\n".into(),
            page_separator: "\n\n".into(),
            layout: RenderLayout::GlyphsHorizontal,
            scale_x: 1,
            scale_y: 1,
        },
    )
    .unwrap();
    assert_eq!(rendered, format!("{via_library}\n"));
    fs::remove_dir_all(path.parent().unwrap()).ok();
}

#[test]
fn cli_render_text_rejects_multiple_subjects() {
    let doc = sample_doc();
    let path = temp_path("render-conflict");
    fs::write(&path, fontspace_json::save(&doc)).unwrap();

    let status = Command::new(env!("CARGO_BIN_EXE_fontspace"))
        .args([
            "render-text",
            path.to_str().unwrap(),
            "--glyph-set",
            "gs",
            "--text",
            "A",
            "--glyphs",
            "0x41",
        ])
        .status()
        .unwrap();
    assert!(
        !status.success(),
        "combining --text and --glyphs must be rejected"
    );
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
fn cli_new_refuses_to_overwrite() {
    let path = temp_path("new-exists");
    fs::write(&path, "existing content").unwrap();
    let status = Command::new(env!("CARGO_BIN_EXE_fontspace"))
        .args(["new", path.to_str().unwrap(), "--name", "X"])
        .status()
        .unwrap();
    assert!(
        !status.success(),
        "new must refuse to overwrite an existing file"
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), "existing content");
    fs::remove_dir_all(path.parent().unwrap()).ok();
}

#[test]
fn cli_set_pixels_rejects_multi_page_selector() {
    let doc = sample_doc(); // has two pages
    let path = temp_path("setpx-multi");
    let original = fontspace_json::save(&doc);
    fs::write(&path, &original).unwrap();
    // --page all resolves to two pages; set-pixels must reject it, not narrow.
    let status = Command::new(env!("CARGO_BIN_EXE_fontspace"))
        .args([
            "set-pixels",
            path.to_str().unwrap(),
            "--glyph-set",
            "gs",
            "--page",
            "all",
            "--code",
            "0x41",
            "--pixel",
            "0,0,1",
        ])
        .status()
        .unwrap();
    assert!(!status.success(), "multi-page --page must be rejected");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
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

#[test]
fn cli_extract_matches_library_extract() {
    let doc = sample_doc();

    // Library: extract all glyphs from Regular and serialize the fragment canonically.
    let glyph_set_id = doc.glyph_sets[0].id;
    let page_id = doc.glyph_sets[0].pages[0].id;
    let fragment = extract_glyphs(
        &doc,
        &ExtractGlyphs {
            glyph_set_id,
            page_id,
            glyphs: GlyphSelector::All,
        },
    )
    .unwrap();
    let expected = fontspace_json::save_fragment(&FontSpaceFragment::Glyphs(fragment));

    // CLI: write the doc, run extract through the binary, read the fragment file.
    let path = temp_path("extract");
    fs::write(&path, fontspace_json::save(&doc)).unwrap();
    let out = path.parent().unwrap().join("fragment.json");
    let status = Command::new(env!("CARGO_BIN_EXE_fontspace"))
        .args([
            "extract",
            path.to_str().unwrap(),
            "--glyph-set",
            "gs",
            "--page",
            "Regular",
            "--output",
            out.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(status.success());

    let via_cli = fs::read_to_string(&out).unwrap();
    assert_eq!(
        via_cli, expected,
        "CLI extract must match library extract byte-for-byte"
    );
    fs::remove_dir_all(path.parent().unwrap()).ok();
}

#[test]
fn cli_extract_dry_run_writes_nothing() {
    let doc = sample_doc();
    let path = temp_path("extract-dry");
    fs::write(&path, fontspace_json::save(&doc)).unwrap();
    let out = path.parent().unwrap().join("fragment.json");

    let output = Command::new(env!("CARGO_BIN_EXE_fontspace"))
        .args([
            "--dry-run",
            "extract",
            path.to_str().unwrap(),
            "--glyph-set",
            "gs",
            "--page",
            "Regular",
            "--output",
            out.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(!out.exists(), "--dry-run must not write the fragment file");
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("dry-run")
    );
    fs::remove_dir_all(path.parent().unwrap()).ok();
}

#[test]
fn cli_extract_rejects_a_non_unique_page() {
    // `--page all` resolves to two pages; extract targets exactly one, so it fails
    // without writing (CLAUDE.md "no hidden remapping").
    let doc = sample_doc();
    let path = temp_path("extract-ambiguous");
    fs::write(&path, fontspace_json::save(&doc)).unwrap();
    let out = path.parent().unwrap().join("fragment.json");

    let status = Command::new(env!("CARGO_BIN_EXE_fontspace"))
        .args([
            "extract",
            path.to_str().unwrap(),
            "--glyph-set",
            "gs",
            "--page",
            "all",
            "--output",
            out.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(!status.success(), "an ambiguous --page must exit non-zero");
    assert!(!out.exists(), "a rejected extract must not write");
    fs::remove_dir_all(path.parent().unwrap()).ok();
}

/// Builds a fragment (all glyphs from Regular) and a destination doc whose Regular
/// page is empty but shares the source geometry/charset — the setup for paste tests.
fn paste_fixture() -> (fontspace_model::GlyphFragment, FontSpace) {
    let src = sample_doc();
    let glyph_set_id = src.glyph_sets[0].id;
    let page_id = src.glyph_sets[0].pages[0].id;
    let fragment = extract_glyphs(
        &src,
        &ExtractGlyphs {
            glyph_set_id,
            page_id,
            glyphs: GlyphSelector::All,
        },
    )
    .unwrap();
    let mut dest = sample_doc();
    dest.glyph_sets[0].pages[0].glyphs.clear(); // empty Regular page
    (fragment, dest)
}

#[test]
fn cli_paste_matches_library_paste() {
    let (fragment, dest) = paste_fixture();
    let glyph_set_id = dest.glyph_sets[0].id;
    let page_id = dest.glyph_sets[0].pages[0].id;

    // Library: paste by code into the empty destination and save canonically.
    let mut via_library = dest.clone();
    paste_glyphs(
        &mut via_library,
        &PasteGlyphs {
            fragment: fragment.clone(),
            target_glyph_set_id: glyph_set_id,
            target_page_id: page_id,
            mapping: GlyphMapping::ByCode,
            size_conversion: GlyphSizeConversion::RequireExact,
        },
    )
    .unwrap();
    let expected = fontspace_json::save(&via_library);

    // CLI: write the destination + fragment file, run paste through the binary.
    let path = temp_path("paste");
    fs::write(&path, fontspace_json::save(&dest)).unwrap();
    let frag_path = path.parent().unwrap().join("fragment.json");
    fs::write(
        &frag_path,
        fontspace_json::save_fragment(&FontSpaceFragment::Glyphs(fragment)),
    )
    .unwrap();
    let status = Command::new(env!("CARGO_BIN_EXE_fontspace"))
        .args([
            "paste",
            path.to_str().unwrap(),
            "--fragment",
            frag_path.to_str().unwrap(),
            "--glyph-set",
            "gs",
            "--page",
            "Regular",
        ])
        .status()
        .unwrap();
    assert!(status.success());
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        expected,
        "CLI paste must match library paste byte-for-byte"
    );
    fs::remove_dir_all(path.parent().unwrap()).ok();
}

#[test]
fn cli_paste_sequential_mapping_matches_library() {
    let (fragment, dest) = paste_fixture();
    let glyph_set_id = dest.glyph_sets[0].id;
    let page_id = dest.glyph_sets[0].pages[0].id;

    // The only fragment glyph is 0x41; sequential-from-code:0x42 remaps it to 0x42.
    let mut via_library = dest.clone();
    paste_glyphs(
        &mut via_library,
        &PasteGlyphs {
            fragment: fragment.clone(),
            target_glyph_set_id: glyph_set_id,
            target_page_id: page_id,
            mapping: GlyphMapping::SequentialFromCode(0x42),
            size_conversion: GlyphSizeConversion::RequireExact,
        },
    )
    .unwrap();
    let expected = fontspace_json::save(&via_library);

    let path = temp_path("paste-seq");
    fs::write(&path, fontspace_json::save(&dest)).unwrap();
    let frag_path = path.parent().unwrap().join("fragment.json");
    fs::write(
        &frag_path,
        fontspace_json::save_fragment(&FontSpaceFragment::Glyphs(fragment)),
    )
    .unwrap();
    let status = Command::new(env!("CARGO_BIN_EXE_fontspace"))
        .args([
            "paste",
            path.to_str().unwrap(),
            "--fragment",
            frag_path.to_str().unwrap(),
            "--glyph-set",
            "gs",
            "--page",
            "Regular",
            "--mapping",
            "sequential-from-code:0x42",
        ])
        .status()
        .unwrap();
    assert!(status.success());
    assert_eq!(fs::read_to_string(&path).unwrap(), expected);
    fs::remove_dir_all(path.parent().unwrap()).ok();
}

#[test]
fn cli_paste_dry_run_writes_nothing() {
    let (fragment, dest) = paste_fixture();
    let path = temp_path("paste-dry");
    let original = fontspace_json::save(&dest);
    fs::write(&path, &original).unwrap();
    let frag_path = path.parent().unwrap().join("fragment.json");
    fs::write(
        &frag_path,
        fontspace_json::save_fragment(&FontSpaceFragment::Glyphs(fragment)),
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_fontspace"))
        .args([
            "--dry-run",
            "paste",
            path.to_str().unwrap(),
            "--fragment",
            frag_path.to_str().unwrap(),
            "--glyph-set",
            "gs",
            "--page",
            "Regular",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        original,
        "--dry-run must not modify the document"
    );
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("dry-run")
    );
    fs::remove_dir_all(path.parent().unwrap()).ok();
}

#[test]
fn cli_paste_missing_fragment_names_the_path() {
    let doc = sample_doc();
    let path = temp_path("paste-missing");
    fs::write(&path, fontspace_json::save(&doc)).unwrap();
    let missing = path.parent().unwrap().join("nope.fragment.json");

    let output = Command::new(env!("CARGO_BIN_EXE_fontspace"))
        .args([
            "paste",
            path.to_str().unwrap(),
            "--fragment",
            missing.to_str().unwrap(),
            "--glyph-set",
            "gs",
            "--page",
            "Regular",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("nope.fragment.json"),
        "the error must name the missing file: {stderr}"
    );
    fs::remove_dir_all(path.parent().unwrap()).ok();
}

#[test]
fn cli_paste_size_conversion_matches_library() {
    let (fragment, dest) = paste_fixture();
    let glyph_set_id = dest.glyph_sets[0].id;
    let page_id = dest.glyph_sets[0].pages[0].id;

    // PlaceAt {1, 1} shifts the fragment's content down-right by one (same geometry).
    let mut via_library = dest.clone();
    paste_glyphs(
        &mut via_library,
        &PasteGlyphs {
            fragment: fragment.clone(),
            target_glyph_set_id: glyph_set_id,
            target_page_id: page_id,
            mapping: GlyphMapping::ByCode,
            size_conversion: GlyphSizeConversion::PlaceAt { x: 1, y: 1 },
        },
    )
    .unwrap();
    let expected = fontspace_json::save(&via_library);

    let path = temp_path("paste-size");
    fs::write(&path, fontspace_json::save(&dest)).unwrap();
    let frag_path = path.parent().unwrap().join("fragment.json");
    fs::write(
        &frag_path,
        fontspace_json::save_fragment(&FontSpaceFragment::Glyphs(fragment)),
    )
    .unwrap();
    let status = Command::new(env!("CARGO_BIN_EXE_fontspace"))
        .args([
            "paste",
            path.to_str().unwrap(),
            "--fragment",
            frag_path.to_str().unwrap(),
            "--glyph-set",
            "gs",
            "--page",
            "Regular",
            "--size",
            "place-at:1,1",
        ])
        .status()
        .unwrap();
    assert!(status.success());
    assert_eq!(fs::read_to_string(&path).unwrap(), expected);
    fs::remove_dir_all(path.parent().unwrap()).ok();
}

/// Runs `fontspace add-export-config` on `path`, adding a row-scan config named
/// `config` over the "Regular" page with `code_bits` code bits.
fn add_export_config(path: &std::path::Path, config: &str, code_bits: &str) {
    let status = Command::new(env!("CARGO_BIN_EXE_fontspace"))
        .args([
            "add-export-config",
            path.to_str().unwrap(),
            "--name",
            config,
            "--glyph-set",
            "gs",
            "--pages",
            "Regular",
            "--code-bits",
            code_bits,
            "--seq",
        ])
        .status()
        .unwrap();
    assert!(status.success(), "add-export-config failed");
}

#[test]
fn cli_export_pads_to_output_size_with_fill_byte() {
    // add-export-config --output-address-bits/--fill-byte persist; export pads the natural
    // image up to 2^bits bytes with the fill byte (spec/10 §10.9).
    let doc = sample_doc();
    let path = temp_path("export-padded");
    fs::write(&path, fontspace_json::save(&doc)).unwrap();
    let status = Command::new(env!("CARGO_BIN_EXE_fontspace"))
        .args([
            "add-export-config",
            path.to_str().unwrap(),
            "--name",
            "rom",
            "--glyph-set",
            "gs",
            "--pages",
            "Regular",
            "--code-bits",
            "7",
            "--output-address-bits",
            "14", // 2^14 = 16384
            "--fill-byte",
            "0xEE",
            "--seq",
        ])
        .status()
        .unwrap();
    assert!(status.success());

    let bin = path.parent().unwrap().join("rom.bin");
    let out = Command::new(env!("CARGO_BIN_EXE_fontspace"))
        .args([
            "export",
            path.to_str().unwrap(),
            "--config",
            "rom",
            "--output",
            bin.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let bytes = fs::read(&bin).unwrap();
    assert_eq!(bytes.len(), 16384, "padded to the configured size");
    // The natural 8×8 / 128-code image is 1024 bytes (3 row + 7 code address bits);
    // everything past it is fill.
    assert!(
        bytes[1024..].iter().all(|&b| b == 0xEE),
        "the padding is the fill byte"
    );
    fs::remove_dir_all(path.parent().unwrap()).ok();
}

#[test]
fn cli_export_matches_library_export() {
    use fontspace_export::{encode_raw_binary, generate_image, row_scan_config, validate_export};
    use fontspace_model::Limits;

    let doc = sample_doc();
    // Library: the same row-scan config over the Regular page → raw bytes. The image is
    // id-independent, so a fresh generator is fine.
    let mut ids = SequentialIdGen::new();
    let gs = &doc.glyph_sets[0];
    let regular = gs.pages[0].id;
    let config = row_scan_config(&mut ids, "rom", gs, vec![regular], 7);
    let limits = Limits::default();
    let summary = validate_export(gs, &config, &limits).unwrap();
    let expected = encode_raw_binary(
        &generate_image(gs, &config, &limits).unwrap(),
        summary.data_bits,
    );

    // CLI: add the config, then export.
    let path = temp_path("export");
    fs::write(&path, fontspace_json::save(&doc)).unwrap();
    add_export_config(&path, "rom", "7");
    let bin = path.parent().unwrap().join("rom.bin");
    let status = Command::new(env!("CARGO_BIN_EXE_fontspace"))
        .args([
            "export",
            path.to_str().unwrap(),
            "--config",
            "rom",
            "--output",
            bin.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(status.success());

    let via_cli = fs::read(&bin).unwrap();
    assert_eq!(via_cli, expected, "CLI export must match library export");
    // 3 row bits + 7 code bits + 0 page bits = 10 address bits → 1024 one-byte words.
    assert_eq!(via_cli.len(), 1024);
    fs::remove_dir_all(path.parent().unwrap()).ok();
}

#[test]
fn cli_validate_export_prints_the_summary() {
    let doc = sample_doc();
    let path = temp_path("validate-export");
    fs::write(&path, fontspace_json::save(&doc)).unwrap();
    add_export_config(&path, "rom", "7");

    let out = Command::new(env!("CARGO_BIN_EXE_fontspace"))
        .args(["validate-export", path.to_str().unwrap(), "--config", "rom"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("Valid 1:1 export"), "{text}");
    assert!(text.contains("128 codes (7 code bits)"), "{text}");
    assert!(text.contains("1024 output bytes"), "{text}");
    fs::remove_dir_all(path.parent().unwrap()).ok();
}

#[test]
fn cli_export_dry_run_writes_no_binary() {
    let doc = sample_doc();
    let path = temp_path("export-dryrun");
    fs::write(&path, fontspace_json::save(&doc)).unwrap();
    add_export_config(&path, "rom", "7");

    let bin = path.parent().unwrap().join("rom.bin");
    let out = Command::new(env!("CARGO_BIN_EXE_fontspace"))
        .args([
            "export",
            path.to_str().unwrap(),
            "--config",
            "rom",
            "--output",
            bin.to_str().unwrap(),
            "--dry-run",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(!bin.exists(), "--dry-run must not write the .bin");
    fs::remove_dir_all(path.parent().unwrap()).ok();
}

#[test]
fn cli_export_dry_run_needs_no_output() {
    // Regression: `--dry-run` writes nothing, so it must not require `--output`.
    let doc = sample_doc();
    let path = temp_path("export-dryrun-no-output");
    fs::write(&path, fontspace_json::save(&doc)).unwrap();
    add_export_config(&path, "rom", "7");

    let out = Command::new(env!("CARGO_BIN_EXE_fontspace"))
        .args([
            "export",
            path.to_str().unwrap(),
            "--config",
            "rom",
            "--dry-run",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "dry-run without --output must succeed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("dry-run"),
        "prints the dry-run summary"
    );
    fs::remove_dir_all(path.parent().unwrap()).ok();
}

#[test]
fn cli_export_without_output_or_dry_run_errors_cleanly() {
    // Writing for real still needs a destination — a clear error, not a clap usage dump.
    let doc = sample_doc();
    let path = temp_path("export-no-output");
    fs::write(&path, fontspace_json::save(&doc)).unwrap();
    add_export_config(&path, "rom", "7");

    let out = Command::new(env!("CARGO_BIN_EXE_fontspace"))
        .args(["export", path.to_str().unwrap(), "--config", "rom"])
        .output()
        .unwrap();
    assert!(!out.status.success(), "a real export needs --output");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("--output is required"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    fs::remove_dir_all(path.parent().unwrap()).ok();
}

#[test]
fn cli_export_rejects_a_non_1to1_config_without_writing() {
    let doc = sample_doc();
    let path = temp_path("export-invalid");
    fs::write(&path, fontspace_json::save(&doc)).unwrap();
    // 6 code bits addresses only 0..63, but 'A' (0x41 = 65) is drawn → not 1:1.
    add_export_config(&path, "rom", "6");

    let bin = path.parent().unwrap().join("rom.bin");
    let out = Command::new(env!("CARGO_BIN_EXE_fontspace"))
        .args([
            "export",
            path.to_str().unwrap(),
            "--config",
            "rom",
            "--output",
            bin.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        !out.status.success(),
        "export of a non-1:1 config must fail"
    );
    assert!(!bin.exists(), "no .bin is written on a validation failure");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("0x0041") || stderr.contains("addressable"),
        "{stderr}"
    );
    fs::remove_dir_all(path.parent().unwrap()).ok();
}
