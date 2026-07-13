# 15. Testing Strategy

The hybrid strict/pragmatic discipline is defined in [../CLAUDE.md](../CLAUDE.md). This chapter lists what must be covered and by which tool. Determinism is mandatory: no real UUIDs, no `now()`, no unseeded random in tests — wire `SequentialIdGen` and fixed constants (chapter 3). Where these tests run (pre-commit hook vs the CI gate, and the OS matrix that compiles the `cfg`-gated platform code) is specified in [19-ci-and-hooks.md](19-ci-and-hooks.md).

## 15.1 Unit tests (default, minimum bar)

- `Bitmap` get/set/toggle/clear/clone/equality;
- row packing for widths not divisible by 8, and the padding-bit-zero invariant;
- shifts in all directions; `Discard` and `Wrap` overflow;
- the line interpolation used by drag editing;
- selector resolution (including ambiguous-name rejection and code/ordinal resolution);
- guide validation and fresh-id-on-copy;
- **character-set edit semantics:** reorder touches no glyphs; add is a no-op; recode warns and moves nothing; remove cascade-deletes and the `ChangeSet` inverts exactly;
- visual-row JSON parse/format and blank-glyph pruning;
- fragment extraction and paste mapping (by code, by slot);
- address-bit decoding; data-bit evaluation; logical memory-image generation;
- id injection determinism.

## 15.2 Round-trip and canonical tests

For representative documents: `domain -> stored JSON -> domain` preserves semantic equality, and `save(load(save(d))) == save(d)` byte-for-byte. Include a document whose page is sparse (proving prune-on-save and absent-as-blank) and one with a dangling glyph (proving tolerate-and-warn).

## 15.3 Golden-file tests

Checked-in examples, regenerated deliberately and diffed before commit:

- blank ASCII 8×16 document (proves an all-blank page stores an empty glyph array);
- multi-page glyph set;
- multiple geometries in one file;
- config-only and mixed glyph/config documents;
- a valid ROM export config and its expected raw memory image (byte-exact);
- intentionally invalid files with expected diagnostics;
- expected text-grid renderings;
- old-`format_version` inputs with expected migrated output.

## 15.4 Property tests (`proptest`)

- `shift(b, 0, 0)` is identity; `flip` twice is identity; `invert` twice is identity;
- setting a pixel to its current value is a semantic no-op;
- JSON round-trip preserves valid random documents;
- extract-then-paste into a compatible empty destination reproduces the selected content;
- reordering a character set never changes any glyph's pixels;
- address evaluation is deterministic;
- a validated export config never triggers an out-of-range bitmap access.

## 15.5 Integration tests

- create file → add ASCII set → add 8×16 glyph set → edit a glyph → save → reopen;
- open two files → copy a page → paste → save destination;
- compare one code across multiple files;
- batch-shift from the CLI and assert the JSON diff;
- the same operation via library, CLI, and MCP produces identical results;
- restore a workspace with multiple documents and tiles (including a cross-document binding);
- render a valid ROM image and compare expected bytes.

## 15.6 GUI tests

Test state transitions beneath the renderer, plus `egui_kittest` snapshots for key views. Essential behaviors: first-pixel stroke mode; drag interpolation; one undo entry per stroke; tile persistence/restore; cross-file drag/copy/paste; page/code selection sync; export-validation feedback. Snapshot review is mandatory (view every changed `.png` and `*.diff.png`).

**Single canonical renderer.** `egui_kittest` snapshots rasterize through `wgpu`, and GPU text/AA output differs between backends (macOS Metal vs Linux lavapipe), so snapshots are **pinned to Linux** — the test file is `#![cfg(target_os = "linux")]`, the macOS CI leg and local macOS dev skip them, and there are no per-OS baselines. Baselines are regenerated deterministically in an `ubuntu:24.04` + lavapipe container matching the `ubuntu-latest` runner (chapter 19 §19.3), via `UPDATE_SNAPSHOTS=1 cargo test`; a modest `failed_pixel_count_threshold` absorbs mesa minor-version AA jitter while still catching real layout changes. The generated `.png` is reviewed and committed; `*.new.png`/`*.diff.png`/`*.old.png` are git-ignored.

## 15.7 Fuzzing (later milestones)

Fuzz the JSON loader, fragment loader, visual-row parser, export evaluator, and migration code. Malformed input must never panic or allocate without reasonable bounds.
