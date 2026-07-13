# FontSpace Implementation Plan

This plan turns [SPEC.md](SPEC.md) into an ordered sequence of shippable milestones. Each milestone is independently useful, ends green (fmt + clippy + tests), and lands as a series of small PRs per the repo protocol ([../CLAUDE.md](../CLAUDE.md)). The numbered acceptance criteria in parentheses map to the release criteria in §16 below.

## Guiding sequencing decisions

- **Core before GUI.** The library + CLI prove the "same operations everywhere" principle cheaply and give the GUI a tested foundation. No `egui` code lands until Milestone 2.
- **Determinism from commit one.** `IdGen` injection and canonical JSON are in the first model/JSON PRs, not retrofitted. Everything downstream depends on them ([spec/03](spec/03-domain-model.md), [spec/06](spec/06-json-persistence.md)).
- **Export last.** The ROM pipeline (Milestone 5) is self-contained and slots in when Glyph-80's Phase-2 hardware needs a font EEPROM. Its schema is defined now but only the strict 1:1 slice is implemented.
- **Vertical slices within a milestone.** Prefer a thin end-to-end path (one op → JSON → CLI → test) over building a whole crate before anything runs.

## Milestone 0 — Workspace bootstrap

Goal: an empty but disciplined Cargo workspace.

- `FontSpace/Cargo.toml` virtual workspace; empty `fontspace-model` and `fontspace-cli` crates; `#![forbid(unsafe_code)]` in each.
- `rust-toolchain.toml` (pin stable), `rustfmt.toml`, `.gitignore` already covers `target/`.
- CI workflow producing the named checks `fmt`, `clippy`, `test` (so the `main` branch rule can require them — see the repo README note). Wire this and tell the user to add the contexts to branch protection.
- Definition of done: `cargo build/test/clippy/fmt` succeed on an empty tree; CI is green.

## Milestone 1 — Core model, JSON, and CLI (no GUI)

Goal: create, edit, validate, round-trip, and render fonts headlessly. (Criteria 1, 3, 4, 13, 14, 19, 20.)

Crates: `fontspace-model`, `fontspace-ops`, `fontspace-json`, `fontspace-render` (text-grid only), `fontspace-cli`.

Steps:

1. **Value types + IDs + injection** — `GlyphSize`, typed IDs, `IdGen`/`RandomIdGen`/`SequentialIdGen`, `Rgba`, `Limits` ([spec/03](spec/03-domain-model.md), [spec/16](spec/16-performance-safety-limits.md)).
2. **`Bitmap`** — packing, padding-bit invariant, get/set/toggle/clear/is_blank/count_on, pure `shifted`/`flipped`/`inverted` ([spec/05](spec/05-glyphs-and-bitmaps.md)). *Strict tests first, incl. odd widths.*
3. **Character sets** — `CharacterEntry { code, label }`, `code`-uniqueness, ordering, ASCII preset op ([spec/04](spec/04-character-sets.md)).
4. **Glyph sets, pages, glyphs, guides** — sparse pages keyed by `code`, `glyph_by_code`, referential-integrity/uniqueness validation ([spec/03](spec/03-domain-model.md), [spec/04](spec/04-character-sets.md)).
5. **JSON layer** — storage structs, visual-row parse/format, canonical writer (deterministic), blank pruning, `format_version` dispatch, migration scaffold. *Round-trip + `save(load(save))==save` + golden tests* ([spec/06](spec/06-json-persistence.md), [spec/15](spec/15-testing.md)).
6. **Operations + selectors + change sets** — `SetPixels`, `ShiftGlyphs`, `ClearGlyphs`, `InvertGlyphs`, page ops, guide ops, and the character-set edit ops incl. **remove-cascade** and **recode-warn**; atomic apply; invertible `ChangeSet` ([spec/07](spec/07-operations.md)). *Strict tests for cascade + inversion.*
7. **Text-grid render** ([spec/09](spec/09-rendering.md)).
8. **CLI** — `new`, `info`, `set-pixels`, `shift`, `render-text`, `extract`; atomic writes; `--dry-run`; JSON errors ([spec/13](spec/13-cli-and-mcp.md)).

Definition of done: the Milestone-1 integration test (create → edit → save → reopen → render, via library and CLI, byte-identical) passes; property tests green; a committed golden ASCII document diffs cleanly.

## Milestone 2 — Single-document editor GUI

Goal: a usable one-file editor. (Criteria 5, 6, 7, 8, 9, 11.)

Crate: `fontspace-egui` (+ `fontspace-render` image output).

Steps:

1. App shell + `egui_tiles` scaffold + default layout + reset-to-default ([spec/12](spec/12-gui.md)).
2. **Glyph editor widget** — square-cell layout math, grid levels, guides overlay, hover; **pure** stroke/interpolation/cell-size functions with unit tests ([spec/12](spec/12-gui.md) §12.3–12.4).
3. **First-pixel-determines-stroke** editing → one `SetPixels` per drag → one undo entry; live tentative stroke.
4. Undo/redo (workspace-level stack, single doc for now) ([spec/07](spec/07-operations.md)).
5. Page overview; character-set view (with pre-apply impact for remove/recode); guides UI.
6. Text preview (image render) ([spec/09](spec/09-rendering.md)).
7. Open/Save/Save As/Revert with atomic writes; dirty tracking.
8. `egui_kittest` snapshots for editor, page overview, preview; snapshot review.

Definition of done: draw a glyph, undo/redo the whole stroke, add guides, view a page, preview text, save and reopen — all through the GUI.

## Milestone 3 — Multi-document workspace, fragments, clipboard

Goal: many files open; copy/move across them; session restore. (Criteria 2, 12, 18.)

Steps:

1. `OpenDocument`/`DocumentId`; multiple documents; document browser tree ([spec/11](spec/11-workspace.md), [spec/12](spec/12-gui.md) §12.2).
2. Fragments + `extract`/`paste` ops with explicit `GlyphMapping`/`PageMapping`/`GlyphSizeConversion` ([spec/08](spec/08-fragments-and-clipboard.md)).
3. Clipboard integration (custom fragment MIME + `text/plain` + `image/png`); cross-instance paste.
4. Cross-document `WorkspaceTransaction` (move A→B undoes both sides).
5. Workspace persistence: stable doc keys, tile layout, selections, bindings; debounced atomic save; restore-on-startup incl. binding remap ([spec/11](spec/11-workspace.md)).
6. Preferences scope (theme, colors, recent paths).

Definition of done: open two files, copy a page and a code range between them with undo, restart, and see layout + selections + a cross-document comparison restored.

## Milestone 4 — Comparison, preview depth, polish

Goal: the visualization features that make it a design tool. (Criterion 10; deepens 9, 11.)

Steps:

1. Character-across-pages/files comparison tile: synchronized zoom, source labels, overlay/difference, copy-across ([spec/12](spec/12-gui.md) §12.9).
2. Page-overview selection operations (shift/clear/invert on a range); range copy/paste.
3. Text-preview presets and multi-page mode; missing-glyph policy.
4. Inspector for glyph/page (export inspector arrives with M5).
5. Menus, keyboard bindings, "focus glyph editor", named-layout groundwork.

Definition of done: compare one code across three files, overlay their differences, and copy the reference glyph into another.

## Milestone 5 — Strict 1:1 ROM export

Goal: turn a font into a ROM image. (Criteria 16, 17.)

Crate: `fontspace-export`.

Steps:

1. Export config types (full schema persisted; `transforms` required empty) ([spec/10](spec/10-rom-export.md)).
2. **Code-addressed** address/data maps; `evaluate_output_word`; logical memory-image generation.
3. **1:1 coverage validator** (partition check, no address/data overlap, width match) with the diagnostics in [spec/14](spec/14-validation-and-errors.md). *Golden byte-exact image tests; property test: validated config never reads OOB.*
4. `RawBinary` encoder; CLI `export` + `validate-export`; MCP export tools.
5. GUI: export-config editor, export inspector, export preview (logical image / raw-bytes view).

Definition of done: define an AT28C64-style 8×16, 128-code, 4-page config; validate it; render the 8192-byte image; assert it byte-for-byte in a golden test; export raw binary from the CLI.

## Cross-cutting, every milestone

- Fuzz targets for parsers/evaluators land alongside the code they cover ([spec/15](spec/15-testing.md) §15.7).
- Each PR updates the relevant `spec/*.md` for any normative change and this plan's status.
- Keep `Limits` and error messages honest as new object kinds appear.

## Release acceptance criteria (§16 of the original spec)

A user can:

1. create/open/save/reopen `.fontspace.json`;
2. keep multiple files open;
3. create/edit reusable character sets incl. ASCII;
4. create multiple geometries in one file;
5. create/duplicate/rename/reorder/copy/paste/delete pages;
6. edit pixels with first-pixel-stroke drag;
7. undo/redo a whole drag as one op;
8. add/edit guides;
9. view a whole page;
10. view one code across pages and files;
11. preview sample text;
12. copy/paste glyphs, ranges, pages, glyph sets, character sets, export configs, and components across files;
13. batch-shift a code range across pages;
14. render configurable text grids;
15. render glyphs/pages to an image;
16. define/save a strict 1:1 ROM export config;
17. validate the mapping and render a raw memory image;
18. restore files/layout/selections/comparisons/zoom after restart;
19. run core ops from library and CLI without GUI;
20. produce deterministic, diff-friendly JSON.

Mapping:

- M1 → 1, 3, 4, 13, 14, 19, 20
- M2 → 5, 6, 7, 8, 9, 11, 15
- M3 → 2, 12, 18
- M4 → 10
- M5 → 16, 17
