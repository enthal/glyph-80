# Claude Code Instructions — FontSpace

This file is canonical for everything under `FontSpace/`. It overrides the repo-root [../CLAUDE.md](../CLAUDE.md) for work in this directory; where the root file adds repo-wide rules (Conventional Commits, branch/PR protocol, markdown no-hardwrap, command governance), those still apply — read both.

## The project

FontSpace is a desktop application **and** a reusable Rust core for designing, organizing, transforming, comparing, rendering, and exporting monospaced raster fonts. It is the first sub-project of Glyph-80 and the tool that produces the glyph bitmaps the later hardware phases display. See [SPEC.md](SPEC.md) and [spec/](spec/) for the full design, and [PLAN.md](PLAN.md) for the milestone sequence.

- **Primary language:** Rust.
- **GUI framework:** `egui` with `egui_tiles`.
- **Canonical document format:** deterministic, human-readable JSON via `serde` / `serde_json`.

The repo is **pre-implementation**. The spec is complete enough to build against; the first feature PRs deliver Milestone 1 (core model + JSON + CLI) as described in [PLAN.md](PLAN.md).

## The spec is the source of truth

- [SPEC.md](SPEC.md) and [spec/*.md](spec/) are canonical. Read the relevant section before writing code that touches an area.
- When code and spec disagree, **raise the conflict with the user** and decide whether to change the code or the spec. Never silently drift.
- Any normative change (a domain type, an invariant, the JSON schema, a selector, an export mapping, an error contract) MUST update the spec in the same commit.
- The load-bearing invariants are consolidated in [spec/17-invariants-and-glossary.md](spec/17-invariants-and-glossary.md). A regression against one of those is a P0 bug.

## The one architectural rule

> UI layers construct and invoke domain operations. They do not own font semantics, transformation logic, serialization rules, or export behavior.

Dependencies flow inward toward the model and operation crates. The core crates (`fontspace-model`, `fontspace-ops`, `fontspace-render`, `fontspace-json`, `fontspace-export`) must **not** depend on `egui`, `egui_tiles`, native dialogs, clipboard/windowing APIs, GUI color/geometry types, or application session state. See [spec/02-architecture.md](spec/02-architecture.md). If you find yourself reaching for a GUI type in a core crate, stop — the boundary is the product.

## Determinism is a first-class requirement

FontSpace's whole persistence story is "readable, diffable JSON in Git." That only works if output is deterministic, which forces two rules that touch nearly every operation:

- **No ambient nondeterminism in core code.** Never call `Uuid::new_v4()`, `Instant::now()`, `SystemTime::now()`, or unseeded RNG inside `fontspace-model`/`ops`/`json`/`export`. IDs and any randomness come from an **injected source** (`IdGen`, and a clock/RNG where needed). Production wires a real UUID-v4 generator; tests wire a `SequentialIdGen` that yields `00000000-0000-0000-0000-000000000001`, `…0002`, … See [spec/03-domain-model.md](spec/03-domain-model.md) §Id injection.
- **Canonical serialization is stable.** Stable field order (explicit storage structs), stable array order, no save-time timestamps, no hash-map iteration order in output, fixed indentation, exactly one trailing newline, lowercase UUIDs. `save(load(save(d))) == save(d)` is a required test. See [spec/06-json-persistence.md](spec/06-json-persistence.md).

## Tests and discipline — hybrid rule

Split by layer, like the rest of Glyph-80's philosophy: the cost of getting core semantics wrong is not the same as the cost of a mis-aligned panel.

### Strict layer — test written FIRST, same commit

Write a failing test **before** the implementation lands; it must fail (or not compile) on the pre-change tree. Applies to:

- `Bitmap` get/set/toggle/clear/clone/equality, and row packing for widths not divisible by 8 (the padding-bit-zero invariant).
- Selector resolution, atomicity, and change-set inversion (undo/redo).
- The character-set edit model: reorder (must not touch glyphs), add (no-op), delete (cascade-delete referencing glyphs, warned), and dangling-glyph tolerance on load.
- Canonical JSON: round-trip, stable re-serialization, visual-row parse/format, blank-glyph pruning on save, versioned migration.
- ROM export: address-bit decoding, data-bit evaluation, logical memory-image generation, and 1:1 coverage validation.
- Any bug fix anywhere: reproduce with a test that fails on the pre-fix tree, then fix.

### Pragmatic layer — test same commit, order doesn't matter

Everything else (tile chrome, inspector rendering, theme, palette wiring, menu glue). **If logic can be tested without a UI, it must not live inside a UI function** — extract it to a pure function and test that. Stroke interpolation, cell-size math, label formatting, hover-coordinate mapping are all pure and belong in testable functions, not in the `egui` paint closure.

### What never moves layers

- "I'll add tests later" means there will be no tests. Both layers ship tests in the same commit.
- A passing test written after the code is a smell — it may assert what the code does, not what it should do.
- **Determinism in tests:** never real UUIDs, `now()`, or unseeded random. Use `SequentialIdGen` and fixed constants. A flaky test is a bug in the test or the code, never "just the runner."

### Test tools (which for which job)

- **Unit** (`#[cfg(test)]`): pure logic — the default and minimum bar.
- **Golden files** (`testdata/`): checked-in `.fontspace.json` documents and expected renderings; regenerate deliberately and `git diff` before committing. Golden files double as the diff-friendliness check.
- **Property** (`proptest`): shift-by-(0,0) is identity, flip-twice is identity, JSON round-trip preserves valid random documents, extract-then-paste into a compatible empty destination reproduces content, validated export never reads out of bounds.
- **Integration** (`tests/`): end-to-end workflows through the library and CLI; assert identical results from both.
- **Snapshot** (`egui_kittest`, later milestones): render a view with deterministic state, compare to a saved `.png`; regenerate with `UPDATE_SNAPSHOTS=1 cargo test` and **visually inspect every changed `.png` and `*.diff.png`** before committing.
- **Fuzz** (later): JSON loader, fragment loader, visual-row parser, export evaluator, migration — malformed input must never panic or allocate unbounded.

## Build & test

Run from the `FontSpace/` directory (it is its own Cargo workspace).

- **Build:** `cargo build --workspace`
- **Test:** `cargo test --workspace`
- **Lint:** `cargo clippy --workspace --all-targets -- -D warnings`
- **Format:** `cargo fmt --all`

Run `cargo fmt`, `cargo clippy`, and `cargo test --workspace` before every commit. Treat clippy warnings as errors.

`fmt` + `clippy` + the markdown no-hardwrap lint run in **both** the developer-installable pre-commit hook and CI; the full `cargo test` suite is the **CI-only gate** (the strict tests-first workflow means failing tests are committed on purpose, so the hook must not block them). CI runs `clippy`/`test` on a macOS + Linux matrix so the `cfg`-gated platform code (chapter 18) is actually checked. Full design: [spec/19-ci-and-hooks.md](spec/19-ci-and-hooks.md).

## Design principles

Operational reminders; the rationale is in the spec.

- **Make wrong states unrepresentable.** Prefer types that cannot express an invalid state over runtime guards. A `Bitmap` exposes only methods that maintain the padding-bit-zero invariant; callers never touch `packed_rows`. Selectors are resolved-and-validated into concrete targets before any mutation begins.
- **Fix bugs structurally, not with guards.** When a bug is stale/inconsistent state across a transition, replace the loose state with a struct updated atomically — don't sprinkle a check at the call site.
- **Atomic or nothing.** A batch op resolves selectors, validates all targets, computes all outputs, applies all changes, and returns one change set. If any target is invalid, the document is unchanged. Cross-document moves undo/redo both sides together.
- **No hidden remapping, resizing, or export heuristics.** Ambiguous paste mappings and size conversions require an explicit policy; the default is `RequireExact`. Never silently alter user data.
- **Pixel meaning is fixed:** `false` = off, `true` = on. Display inversion is a view concern and is never stored in glyph data.
- **UI state is not document data.** Selections, zoom, tile layout, and cross-document bindings live in workspace state, never in the `.fontspace.json` file.

## Code style

- **Typed IDs for everything durable:** `GlyphSetId`, `PageId`, `GuideId`, `ExportConfigId`, `ExportComponentId`, `DocumentId` (runtime-only). These are newtype-wrapped UUIDs, not interchangeable — the type system should say so. **Character entries are the deliberate exception:** they are keyed by their `code: u32` (their identity and ROM address dimension), not a UUID. See [spec/04-character-sets.md](spec/04-character-sets.md).
- **Map naming:** `things_by_key` for a `Map<key, thing>` **field**; "things" plural; a stored `Map<code, glyph>` is `glyphs_by_code`. For nested: `things_by_inner_by_outer` means `Map<outer, Map<inner, thing>>` (read right-to-left). For collection values include the container: `glyph_vecs_by_page`. **`_by_` is for maps only.** A **singular accessor** (`fn …(&self, key) -> Option<&Thing>`) never uses `_by_`; name it for what it returns (`fn glyph(...)`), and if the key must be named use `_of_`: `GlyphPage::glyph_of_code`, `GlyphSet::page_of_id` (spec/03 §3.6). So: map of many → `things_by_key`; accessor of one → `thing_of_key`.
- **No `unsafe`.** Every FontSpace crate sets `#![forbid(unsafe_code)]` at the crate root. If you want `unsafe`, stop and ask — it almost certainly means a different dependency or abstraction.
- **No `unwrap()` / `expect()` in non-test code** except where a contract makes failure impossible, and even then prefer a typed `Result`. `expect` messages describe the invariant, not the operation.
- **Structural safety over incidental correctness:** `char_indices()` over byte slicing; typed newtypes over bare `u64`; access bitmaps through methods, never the packed layout.
- **Duplicate widget IDs are a critical bug.** Any view may render in multiple tiles simultaneously. Salt global `egui` IDs (`Id::new`, `TopBottomPanel::top`, `ScrollArea`, `ComboBox`) with a tile-/document-specific value; inside a tile prefer `ui.id().with("key")`. Before adding an ID, ask: could two instances be on screen at once?
- **Markdown is never hard-wrapped.** One logical line per paragraph, list item, and block-quote — let the renderer soft-wrap. Applies to every `.md` file here and in `spec/`.
- **Errors identify object context.** A load/validation error names document object, glyph set, page, code/slot, row, and column where possible (e.g. `Terminal 8×16 / Regular / code 0x41 / row 7: expected 8 pixels, found 7`). Errors are structured Rust types; GUI/CLI/MCP render them.

## Commit & PR protocol

Follows the repo-root protocol ([../CLAUDE.md](../CLAUDE.md)): Conventional Commits, feature branch → PR → squash merge, never commit to `main`. FontSpace specifics:

- **Scope commits `fontspace`** (or a crate sub-scope): `feat(fontspace): …`, `fix(fontspace-json): …`.
- **Tests-first where the strict layer applies** — the test must have failed on the pre-change tree; mention it in the commit body when useful.
- Before every commit: `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`. Regenerate and visually review any snapshot changes.
- **Spec sync in the same commit** for any normative change. If `CLAUDE.md`, `Cargo.toml`, `Cargo.lock`, or toolchain files changed, include them.
- Small, reviewable commits. A commit touching more than one crate without a clear reason is a signal to split.
