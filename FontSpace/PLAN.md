# FontSpace Implementation Plan

This plan turns [SPEC.md](SPEC.md) into an ordered sequence of shippable milestones. Each milestone is independently useful, ends green (fmt + clippy + tests), and lands as a series of small PRs per the repo protocol ([../CLAUDE.md](../CLAUDE.md)). The numbered acceptance criteria in parentheses map to the release criteria in §16 below.

## Guiding sequencing decisions

- **Core before GUI.** The library + CLI prove the "same operations everywhere" principle cheaply and give the GUI a tested foundation. No `egui` code lands until Milestone 2.
- **Determinism from commit one.** `IdGen` injection and canonical JSON are in the first model/JSON PRs, not retrofitted. Everything downstream depends on them ([spec/03](spec/03-domain-model.md), [spec/06](spec/06-json-persistence.md)).
- **Export last.** The ROM pipeline (Milestone 5) is self-contained and slots in when Glyph-80's Phase-2 hardware needs a font EEPROM. Its schema is defined now but only the strict 1:1 slice is implemented.
- **Vertical slices within a milestone.** Prefer a thin end-to-end path (one op → JSON → CLI → test) over building a whole crate before anything runs.

## Milestone 0 — Workspace bootstrap

Goal: an empty but disciplined Cargo workspace.

- [x] `FontSpace/Cargo.toml` virtual workspace; empty `fontspace-model` and `fontspace-cli` crates; `#![forbid(unsafe_code)]` in each. — [#2](https://github.com/enthal/glyph-80/pull/2)
- [x] `rust-toolchain.toml` (pin stable), `rustfmt.toml`, `.gitignore` already covers `target/`. — [#2](https://github.com/enthal/glyph-80/pull/2)
- [x] **Pre-commit hook** ([spec/19](spec/19-ci-and-hooks.md) §19.2): git hooks are repo-global, so a **repo-root** dispatcher (`.cargo-husky/hooks/pre-commit`) is installed into `.git/hooks/` by **`cargo-husky`** (a `user-hooks` dev-dependency of `fontspace-cli`) on the first `cargo test` — a repo-level, one-time setup since FontSpace is the first sub-project. FontSpace contributes `FontSpace/scripts/precommit.sh` running `fmt` + `clippy` + the markdown no-hardwrap lint (not the test suite), invoked when `FontSpace/**` is staged. A one-line note in the repo-root [../CLAUDE.md](../CLAUDE.md) records that the hook mechanism lives at the root.
- [x] **CI workflow** ([spec/19](spec/19-ci-and-hooks.md) §19.3): `.github/workflows/fontspace-ci.yml`, path-filtered to `FontSpace/**`, with jobs `fmt` (ubuntu), `clippy` + `test` on a **macOS + Linux matrix** (so the `cfg`-gated platform code in chapter 18 is actually checked), and `markdown`. Cache cargo; pin toolchain. — [#2](https://github.com/enthal/glyph-80/pull/2)
- [ ] **Branch protection** ([spec/19](spec/19-ci-and-hooks.md) §19.4): once CI runs, add the six required contexts (`fontspace / fmt`, `fontspace / clippy (…)` ×2, `fontspace / test (…)` ×2, `fontspace / markdown`) to the `main` rule — this closes the item deferred at repo setup. Note the monorepo path-filter gotcha (§19.5) for when a second sub-project lands. **Deferred:** requires GitHub admin (a manual, human step); CI has run green on every PR since [#2](https://github.com/enthal/glyph-80/pull/2).

Definition of done: `cargo build/test/clippy/fmt` succeed on an empty tree; CI is green on the matrix; the hook installs and blocks a mis-formatted commit.

## Milestone 1 — Core model, JSON, and CLI (no GUI)

Goal: create, edit, validate, round-trip, and render fonts headlessly. (Criteria 1, 3, 4, 13, 14, 19, 20.)

Crates: `fontspace-model`, `fontspace-ops`, `fontspace-json`, `fontspace-render` (text-grid only), `fontspace-cli`.

Steps:

- [x] **Value types + IDs + injection** — `GlyphSize`, typed IDs, `IdGen`/`RandomIdGen`/`SequentialIdGen`, `Rgba`, `Limits` ([spec/03](spec/03-domain-model.md), [spec/16](spec/16-performance-safety-limits.md)). — [#3](https://github.com/enthal/glyph-80/pull/3)
- [x] **`Bitmap`** — packing, padding-bit invariant, get/set/toggle/clear/is_blank/count_on, pure `shifted`/`flipped`/`inverted` ([spec/05](spec/05-glyphs-and-bitmaps.md)). *Strict tests first, incl. odd widths.* — [#3](https://github.com/enthal/glyph-80/pull/3)
- [ ] **Character sets** — `CharacterEntry { code, label }`, `code`-uniqueness, ordering, ASCII preset op ([spec/04](spec/04-character-sets.md)). *Data model + `code`-uniqueness + ordering done in [#5](https://github.com/enthal/glyph-80/pull/5); the ASCII preset op (needs `IdGen`) is still pending.*
- [x] **Glyph sets, pages, glyphs, guides** — sparse pages keyed by `code`, `glyph_of_code`, referential-integrity/uniqueness validation ([spec/03](spec/03-domain-model.md), [spec/04](spec/04-character-sets.md)). — [#5](https://github.com/enthal/glyph-80/pull/5)
- [x] **JSON layer** — storage structs, visual-row parse/format, canonical writer (deterministic), blank pruning, `format_version` dispatch, migration scaffold. *Round-trip + `save(load(save))==save` + golden tests* ([spec/06](spec/06-json-persistence.md), [spec/15](spec/15-testing.md)). — [#7](https://github.com/enthal/glyph-80/pull/7)
- [x] **Operations + selectors + change sets** — `SetPixels`, `ShiftGlyphs`, `ClearGlyphs`, `InvertGlyphs`, page ops, guide ops, and the character-set edit ops incl. **remove-cascade** and **recode-warn**; atomic apply; invertible `ChangeSet` ([spec/07](spec/07-operations.md)). *Strict tests for cascade + inversion.* — glyph ops [#8](https://github.com/enthal/glyph-80/pull/8), page + guide ops [#9](https://github.com/enthal/glyph-80/pull/9), charset entry ops [#10](https://github.com/enthal/glyph-80/pull/10), remove-cascade [#11](https://github.com/enthal/glyph-80/pull/11).
- [x] **Text-grid render** ([spec/09](spec/09-rendering.md)). — [#12](https://github.com/enthal/glyph-80/pull/12)
- [x] **CLI** — `new`, `info`, `set-pixels`, `shift`, `render-text`, `extract`; atomic writes; `--dry-run`; JSON errors ([spec/13](spec/13-cli-and-mcp.md)). — [#13](https://github.com/enthal/glyph-80/pull/13) delivers `new`/`info`/`set-pixels`/`shift`/`render-text` with atomic writes, `--dry-run`, and a `--seq` reproducible-id flag; **`extract` defers to Milestone 3** (it needs the fragments crate). Structured errors render to stderr; machine-readable JSON error output is a later refinement.

Definition of done: the Milestone-1 integration test (create → edit → save → reopen → render, via library and CLI, byte-identical) passes; property tests green; a committed golden ASCII document diffs cleanly.

## Milestone 2 — Single-document editor GUI

Goal: a usable one-file editor. (Criteria 5, 6, 7, 8, 9, 11.)

Crate: `fontspace-egui` (+ `fontspace-render` image output).

Steps:

- [x] App shell + `egui_tiles` scaffold + default layout + reset-to-default ([spec/12](spec/12-gui.md)). Also established the `egui_kittest` snapshot infrastructure (Linux-pinned lavapipe renderer, container-baked baselines, CI wgpu deps — spec/15 §15.6, spec/19 §19.3) with the first shell snapshot. — [#14](https://github.com/enthal/glyph-80/pull/14)
- [x] **Glyph editor widget** — square-cell layout math, grid levels, guides overlay, hover; **pure** cell-size/hover/guide functions with unit tests ([spec/12](spec/12-gui.md) §12.3). — [#16](https://github.com/enthal/glyph-80/pull/16) *(display only; the stroke/interpolation pure functions land with the editing slice below, §12.4)*
- [x] **First-pixel-determines-stroke** editing → one `SetPixels` per drag → one undo entry; live tentative stroke. — [#17](https://github.com/enthal/glyph-80/pull/17)
- [x] Undo/redo (workspace-level stack, single doc for now) ([spec/07](spec/07-operations.md)). — [#17](https://github.com/enthal/glyph-80/pull/17)
- [x] Page overview — thumbnail grid, click-to-select, absent-blank, dangling flagged ([spec/12](spec/12-gui.md) §12.8). — [#18](https://github.com/enthal/glyph-80/pull/18)
- [ ] Character-set view (ordered table, with pre-apply impact for remove/recode) ([spec/12](spec/12-gui.md) §12.7).
- [ ] Guides UI (add/edit/drag/show-hide/copy-to-pages) ([spec/12](spec/12-gui.md) §12.6).
- [ ] Text preview (image render) ([spec/09](spec/09-rendering.md)).
- [ ] Open/Save/Save As/Revert with atomic writes; dirty tracking.
- [ ] **Platform identity + Linux desktop integration** ([spec/18](spec/18-platform-support.md)): `APP_ID`/storage-namespace constants, embedded `assets/app_icon.png` → window icon, `with_app_id`, the self-installing `.desktop`+icon (Exec-resolves / AppImage / `StartupWMClass` / `update-desktop-database`), and the `cursor_env` re-exec bridge. Port Termica's pure helpers (`desktop_entry_contents`, `desktop_exec_field`, `resolve_exec_path`) with their unit tests, plus the `APP_ID == packager identifier` test.
- [ ] `egui_kittest` snapshots for editor, page overview, preview; snapshot review. *(Infrastructure landed in [#14](https://github.com/enthal/glyph-80/pull/14); each view slice adds its own snapshot, so this closes when the last view does.)*

Definition of done: draw a glyph, undo/redo the whole stroke, add guides, view a page, preview text, save and reopen — all through the GUI; on Linux the window carries our icon and the app appears in the launcher.

## Milestone 3 — Multi-document workspace, fragments, clipboard

Goal: many files open; copy/move across them; session restore. (Criteria 2, 12, 18.)

Steps:

- [ ] `OpenDocument`/`DocumentId`; multiple documents; document browser tree ([spec/11](spec/11-workspace.md), [spec/12](spec/12-gui.md) §12.2).
- [ ] Fragments + `extract`/`paste` ops with explicit `GlyphMapping`/`PageMapping`/`GlyphSizeConversion` ([spec/08](spec/08-fragments-and-clipboard.md)).
- [ ] Clipboard integration (custom fragment MIME + `text/plain` + `image/png`); cross-instance paste.
- [ ] Cross-document `WorkspaceTransaction` (move A→B undoes both sides).
- [ ] Workspace persistence: stable doc keys, tile layout, selections, bindings; debounced atomic save; restore-on-startup incl. binding remap ([spec/11](spec/11-workspace.md)).
- [ ] Preferences scope (theme, colors, recent paths).

Definition of done: open two files, copy a page and a code range between them with undo, restart, and see layout + selections + a cross-document comparison restored.

## Milestone 4 — Comparison, preview depth, polish

Goal: the visualization features that make it a design tool. (Criterion 10; deepens 9, 11.)

Steps:

- [ ] Character-across-pages/files comparison tile: synchronized zoom, source labels, overlay/difference, copy-across ([spec/12](spec/12-gui.md) §12.9).
- [ ] Page-overview selection operations (shift/clear/invert on a range); range copy/paste.
- [ ] Text-preview presets and multi-page mode; missing-glyph policy.
- [ ] Inspector for glyph/page (export inspector arrives with M5).
- [ ] **Menu system** ([spec/18](spec/18-platform-support.md) §18.3–18.4): the one command-registry tree; the in-window `egui` presenter; the native macOS `NSMenu` presenter (`muda`) with winit default-menu suppression, creator-callback install timing, `Box::leak` lifetime, and `MenuEvent` routing for the full menu bar; keyboard bindings kept in sync with the native accelerators; "focus glyph editor"; named-layout groundwork.

Definition of done: compare one code across three files, overlay their differences, and copy the reference glyph into another.

## Milestone 5 — Strict 1:1 ROM export

Goal: turn a font into a ROM image. (Criteria 16, 17.)

Crate: `fontspace-export`.

Steps:

- [ ] Export config types (full schema persisted; `transforms` required empty) ([spec/10](spec/10-rom-export.md)).
- [ ] **Code-addressed** address/data maps; `evaluate_output_word`; logical memory-image generation.
- [ ] **1:1 coverage validator** (partition check, no address/data overlap, width match) with the diagnostics in [spec/14](spec/14-validation-and-errors.md). *Golden byte-exact image tests; property test: validated config never reads OOB.*
- [ ] `RawBinary` encoder; CLI `export` + `validate-export`; MCP export tools.
- [ ] GUI: export-config editor, export inspector, export preview (logical image / raw-bytes view).

Definition of done: define an AT28C64-style 8×16, 128-code, 4-page config; validate it; render the 8192-byte image; assert it byte-for-byte in a golden test; export raw binary from the CLI.

## Cross-cutting, every milestone

- [ ] Fuzz targets for parsers/evaluators land alongside the code they cover ([spec/15](spec/15-testing.md) §15.7).
- [ ] Each PR updates the relevant `spec/*.md` for any normative change and this plan's status.
- [ ] Keep `Limits` and error messages honest as new object kinds appear.

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
