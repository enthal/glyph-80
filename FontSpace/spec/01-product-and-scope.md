# 1. Product and Scope

## 1.1 Product definition

FontSpace is a desktop application and a reusable Rust core for designing, organizing, transforming, comparing, rendering, and exporting monospaced raster fonts.

A FontSpace document is not limited to one font, one glyph size, one character set, or one export configuration. It is a general container for related font-design objects:

- zero or more character sets;
- zero or more glyph sets;
- zero or more pages within each glyph set;
- zero or more ROM/export configurations;
- metadata for the document and its contained objects.

The application supports multiple FontSpace files open simultaneously. Users can compare, copy, move, combine, and transform glyphs, pages, glyph sets, character sets, and export-configuration objects across files.

The product exposes the same core operations through multiple front ends: the `egui` desktop UI, a command-line interface, an MCP/agent-facing tool interface, direct Rust library calls, and tests or one-off scripts. The core design principle that governs everything: **UI layers construct and invoke domain operations; they do not own font semantics, transformation logic, serialization rules, or export behavior.**

## 1.2 Goals

- simple editing of binary raster glyphs;
- reusable named character sets;
- multiple glyph geometries in one file;
- multiple pages per glyph set;
- named horizontal and vertical guides per page;
- flexible cross-file copy and paste;
- whole-page, cross-page, and text-based font visualization;
- deterministic JSON files suitable for Git version control and readable diffs;
- reusable, saved ROM/export configurations;
- a strict initial 1:1 ROM export implementation;
- an architecture that can later support padding, splitting, tiling, interleaving, and other geometry transformations;
- automatic persistence and restoration of application workspace state;
- clean separation among model, operations, rendering, persistence, workspace state, and UI.

## 1.3 Non-goals for the initial release

- built-in importers for every historical or modern font format;
- a plugin system;
- collaborative editing;
- cloud synchronization;
- arbitrary scripting embedded in the application;
- a fully general expression language for export mappings;
- advanced vector drawing tools;
- implicit or heuristic resizing when copying between incompatible glyph sizes;
- automatic saving of user documents on every edit.

External tools — including AI-generated ones — can convert other font formats into FontSpace JSON. FontSpace's obligation is to make its JSON schema straightforward enough for that to be practical.

## 1.4 Enduring design principles

These hold for v1 and constrain all future work:

- A FontSpace file is a composable object container, not one fixed font.
- Geometry belongs to glyph sets, not globally to a file.
- Character sets are reusable ordered objects; an entry is identified by its `code`, not its position.
- Pages are sparse over their glyph set's character set: a page holds a glyph only for the codes it defines, and every absent code renders blank.
- Pixel meaning is always binary on/off; display inversion is a view concern and is never stored in glyph data.
- UI state is separate from document data.
- Clipboard content is a domain fragment, not a GUI artifact.
- Operations are typed, atomic, testable, and reusable across GUI, CLI, MCP, and scripts.
- ROM export is a pipeline: transform → pack → map → render → encode. v1 implements only the strict 1:1 slice, but leaves clean extension points.
- No hidden remapping, resizing, or export heuristic ever alters user data silently.
- JSON output is deterministic and Git-friendly.

## 1.5 Relationship to Glyph-80

FontSpace is Glyph-80's first sub-project and its tooling foundation: it produces the glyph bitmaps that the hardware phases display. The ROM export model (chapter 10) is designed so that a `.fontspace.json` can drive the Phase-2 font EEPROM directly — the ROM is addressed by character `code`, exactly as the display hardware addresses it. See the roadmap in [../../README.md](../../README.md).
