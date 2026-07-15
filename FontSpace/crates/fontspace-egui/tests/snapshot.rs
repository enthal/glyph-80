//! `egui_kittest` snapshot tests for the GUI shell (spec/15 §15.6).
//!
//! **Pinned to Linux (lavapipe) as the single canonical renderer.** GPU text
//! rasterization differs between macOS Metal and Linux lavapipe, so a PNG baked on
//! one backend will not match another. Rather than carry per-OS baselines, the
//! snapshot tests run only on Linux; the macOS CI matrix and local macOS dev skip
//! them (the `#![cfg(target_os = "linux")]` below makes the file empty elsewhere).
//!
//! Baselines are generated in an `ubuntu:24.04` + lavapipe container that matches
//! the `ubuntu-latest` CI runner, via `UPDATE_SNAPSHOTS=1 cargo test`. Every changed
//! `.png` / `.diff.png` must be reviewed before commit (spec/15 §15.6, CLAUDE.md).
#![cfg(target_os = "linux")]

use std::path::PathBuf;

use egui_kittest::{Harness, SnapshotOptions};
use fontspace_egui::charset_view::show_character_set;
use fontspace_egui::document_browser::show_document_browser;
use fontspace_egui::editor::show_glyph_editor;
use fontspace_egui::page_overview::show_page_overview;
use fontspace_egui::text_preview::show_text_preview;
use fontspace_egui::{AppState, FontSpaceApp, GuardedIntent};
use fontspace_model::SequentialIdGen;

/// A small differing-pixel cushion absorbs mesa/lavapipe minor-version AA jitter
/// between the baseline container and the CI runner, while still catching any real
/// layout change (the default perceptual `threshold` of 0.6 already targets
/// cross-backend robustness).
fn options() -> SnapshotOptions {
    SnapshotOptions::new().failed_pixel_count_threshold(2_000)
}

/// A deterministic starter document (sequential ids — never random in tests).
fn state() -> AppState {
    AppState::with_ids(Box::new(SequentialIdGen::new()))
}

#[test]
fn shell_default_layout() {
    let mut app = FontSpaceApp::with_state(state());
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .wgpu()
        .build_ui(move |ui| app.show(ui));
    harness.run();
    harness.snapshot_options("shell_default_layout", &options());
}

#[test]
fn glyph_editor() {
    let mut state = state();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(360.0, 400.0))
        .wgpu()
        .build_ui(move |ui| show_glyph_editor(ui, &mut state));
    harness.run();
    harness.snapshot_options("glyph_editor", &options());
}

#[test]
fn character_set() {
    let mut state = state();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(360.0, 360.0))
        .wgpu()
        .build_ui(move |ui| show_character_set(ui, &mut state));
    harness.run();
    harness.snapshot_options("character_set", &options());
}

#[test]
fn text_preview() {
    let mut state = state();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(400.0, 220.0))
        .wgpu()
        .build_ui(move |ui| show_text_preview(ui, &mut state));
    harness.run();
    harness.snapshot_options("text_preview", &options());
}

#[test]
fn page_overview() {
    let mut state = state();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(400.0, 300.0))
        .wgpu()
        .build_ui(move |ui| show_page_overview(ui, &mut state));
    harness.run();
    harness.snapshot_options("page_overview", &options());
}

#[test]
fn document_browser() {
    let mut state = state();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(280.0, 420.0))
        .wgpu()
        .build_ui(move |ui| show_document_browser(ui, &mut state));
    harness.run();
    harness.snapshot_options("document_browser", &options());
}

#[test]
fn dirty_titlebar_and_status_strip() {
    // A saved-then-edited document: the menu bar shows the file name with the unsaved
    // marker, and the status strip reports the last save (spec/12 §12.12).
    let mut state = state();
    state.mark_saved(PathBuf::from("/fonts/demo.fontspace.json"));
    state.begin_stroke((0, 0));
    state.commit_stroke();
    let mut app = FontSpaceApp::with_state(state);
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .wgpu()
        .build_ui(move |ui| app.show(ui));
    harness.run();
    harness.snapshot_options("dirty_titlebar_and_status_strip", &options());
}

#[test]
fn discard_confirm_modal() {
    // A dirty document with an Open action pending: the unsaved-changes modal is
    // shown over the workspace (spec/12 §12.12).
    let mut state = state();
    state.begin_stroke((0, 0));
    state.commit_stroke();
    assert!(!state.begin_guarded(GuardedIntent::Open)); // dirty → arms the modal
    let mut app = FontSpaceApp::with_state(state);
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .wgpu()
        .build_ui(move |ui| app.show(ui));
    harness.run();
    harness.snapshot_options("discard_confirm_modal", &options());
}

#[test]
fn glyph_editor_after_stroke() {
    let mut state = state();
    // Apply a committed vertical stroke down the left column, as if dragged, so the
    // snapshot shows editing having changed the glyph.
    state.begin_stroke((0, 0));
    state.extend_stroke((0, 7));
    state.commit_stroke();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(360.0, 400.0))
        .wgpu()
        .build_ui(move |ui| show_glyph_editor(ui, &mut state));
    harness.run();
    harness.snapshot_options("glyph_editor_after_stroke", &options());
}
