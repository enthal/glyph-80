# FontSpace Specification

**Status:** Implementation specification (v1 target)
**Audience:** Implementation agents and maintainers
**Language / GUI / format:** Rust · `egui` + `egui_tiles` · deterministic JSON (`serde`)

FontSpace is a desktop application and reusable Rust core for designing, organizing, transforming, comparing, rendering, and exporting monospaced raster fonts. A FontSpace document is a general container for related font-design objects — character sets, glyph sets, pages, glyphs, guides, and export configurations — not a single fixed font. The same typed operations are exposed through the GUI, a CLI, an MCP/agent interface, direct library calls, and tests.

This file is the table of contents. Each chapter under [spec/](spec/) is canonical for its area. The consolidated, load-bearing invariants live in [spec/17-invariants-and-glossary.md](spec/17-invariants-and-glossary.md) — start there for the non-negotiables.

## Chapters

1. [Product and scope](spec/01-product-and-scope.md) — what FontSpace is, goals, non-goals, and the enduring design principles.
2. [Architecture](spec/02-architecture.md) — the crate layering and the inward-dependency rule.
3. [Domain model](spec/03-domain-model.md) — `FontSpace`, glyph sets, pages, typed IDs, and id/RNG injection.
4. [Character sets](spec/04-character-sets.md) — entries keyed by `code`, ordering, the sparse-page model, and the cascade rules.
5. [Glyphs and bitmaps](spec/05-glyphs-and-bitmaps.md) — the `Bitmap` type, bit packing, the padding invariant, and blank/sparse semantics.
6. [JSON persistence](spec/06-json-persistence.md) — canonical format, visual glyph rows, storage structs, determinism, versioning, and migration.
7. [Operations](spec/07-operations.md) — commands, queries, selectors, atomicity, change sets, undo, and workspace transactions.
8. [Fragments and clipboard](spec/08-fragments-and-clipboard.md) — serializable fragments, clipboard representations, and paste policies.
9. [Rendering](spec/09-rendering.md) — text-grid, image, and text-preview projections.
10. [ROM and programmer export](spec/10-rom-export.md) — export configs, the strict 1:1 v1 constraint, code-addressed maps, evaluation, and the future pipeline.
11. [Multi-document workspace](spec/11-workspace.md) — open documents, cross-document references, and session persistence.
12. [GUI structure and UX](spec/12-gui.md) — the tiled shell, the glyph editor, pointer/keyboard interaction, and the views.
13. [CLI and MCP](spec/13-cli-and-mcp.md) — the thin adapters over typed operations.
14. [Validation and errors](spec/14-validation-and-errors.md) — document, operation, and export validation; error quality.
15. [Testing strategy](spec/15-testing.md) — the layers, determinism, and what must be golden/property/fuzz tested.
16. [Performance, safety, and limits](spec/16-performance-safety-limits.md) — scale, atomic file safety, and resource limits.
17. [Invariants and glossary](spec/17-invariants-and-glossary.md) — the consolidated non-negotiables and shared vocabulary.

## Reading order

- **Building the core (Milestone 1):** 17 → 3 → 4 → 5 → 6 → 7 → 14 → 15.
- **Building the GUI (Milestones 2–4):** 2 → 11 → 12 → 9 → 8.
- **Building export (Milestone 5):** 10 → 14 → 15.

See [PLAN.md](PLAN.md) for the milestone breakdown and sequencing.
