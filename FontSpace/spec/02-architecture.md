# 2. Architecture

FontSpace is a set of layered Rust crates in one Cargo workspace under `FontSpace/`. The exact layout may evolve, but **dependencies flow inward toward the model and operation crates.** A crate may depend only on crates above it in this list.

```text
fontspace-model     Core document and value types; validation; typed IDs; id/RNG injection;
                    fragments and dependency metadata.

fontspace-ops       Commands and queries; pure bitmap operations; selectors; change sets;
                    single- and cross-document transactions.

fontspace-render    Text-grid rendering; image rendering; text-preview projection.
                    Returns buffers/strings, never GUI textures.

fontspace-export    Export configs' evaluation: address/data decoding, logical memory-image
                    generation, 1:1 coverage validation, output encoders.

fontspace-json      Stable JSON schema; load/save; atomic file read/write; canonical formatting; versioned migration.

fontspace-egui      Application shell; egui_tiles workspace; views and inspectors;
                    input-gesture translation; clipboard and dialog integration.

fontspace-cli       CLI parsing; file load/save; invocation of ops/render/export.

fontspace-mcp       Tool schemas; invocation of the same typed operations.
```

`fontspace-render` and `fontspace-export` are siblings that both consume `fontspace-model`/`fontspace-ops`; neither depends on the other. `fontspace-json` depends on `model` (and may reference `ops` for migration helpers). The three front ends — `egui`, `cli`, `mcp` — sit at the top and depend on everything below.

## 2.1 What the core must never depend on

`fontspace-model`, `fontspace-ops`, `fontspace-render`, `fontspace-export`, and `fontspace-json` must not depend on:

- `egui` or `egui_tiles`;
- native file dialogs;
- platform clipboard APIs;
- windowing APIs;
- GUI color or geometry types;
- application session/workspace state.

The GUI crate may depend on all lower crates. The rule is enforced by crate boundaries, not discipline: if a core function needs a color, it takes the core `Rgba` value type (chapter 9), not an `egui::Color32`.

## 2.2 Why the split matters

Every capability must be reachable without constructing a UI. The CLI and MCP adapters are proof: they invoke the exact same `fontspace-ops` functions the GUI does. A behavior that can only be triggered through `egui` is a design bug. This is also what makes the strict-layer tests cheap — they exercise `ops`/`json`/`export` directly, with no renderer or event loop.

## 2.3 Crate-level conventions

- Every crate sets `#![forbid(unsafe_code)]` at its root.
- Public error types are structured (chapter 14), not stringly-typed; front ends render them.
- `fontspace-model` owns the `IdGen` trait and the value types; `fontspace-ops` threads an `IdGen` through every object-creating operation (chapter 3). No core crate calls `Uuid::new_v4()` or a clock directly.
