# 13. CLI and MCP

Both front ends are thin adapters over the typed operations in `fontspace-ops`. Neither reimplements domain logic; both go through the same functions the GUI uses.

## 13.1 CLI

```text
fontspace shift file.fontspace.json \
  --glyph-set "Terminal 8x16" --pages Regular,Bold \
  --glyphs A-Z --dx 1 --dy 0

fontspace render-text file.fontspace.json \
  --glyph-set "Terminal 8x16" --page Regular \
  --glyphs 0x20-0x7E --on "##" --off "  "

fontspace extract file.fontspace.json \
  --glyph-set "Terminal 8x16" --pages Regular,Bold \
  --glyphs A-Z --output fragment.json
```

Glyph ranges accept characters (`A-Z`), decimal, or `0x` hex codes, resolved via the referenced character set. Requirements:

- optional machine-readable (JSON) error output;
- `--dry-run` for mutating operations (prints the resulting `ChangeSet` summary, writes nothing);
- canonical JSON formatting preserved on write;
- **atomic writes** (tmp + rename, chapter 16);
- **fail without modifying files** when validation fails.

The CLI wires the production `IdGen` by default; a hidden/test flag can wire the sequential generator for reproducible fixtures.

## 13.2 MCP

MCP tools expose the same operations with JSON schemas. Example mutation:

```json
{
  "operation": "shift_glyphs",
  "document": "terminal.fontspace.json",
  "glyph_set": "Terminal 8x16",
  "pages": ["Regular", "Bold"],
  "glyphs": { "start_code": "0x41", "end_code": "0x5A" },
  "dx": 1, "dy": 0, "overflow": "discard"
}
```

Example pixel edit:

```json
{
  "operation": "set_pixels",
  "glyph_set_id": "...",
  "page_id": "...",
  "code": "0x41",
  "edits": [ { "x": 3, "y": 2, "value": true }, { "x": 4, "y": 2, "value": true } ]
}
```

MCP must not reimplement domain logic; it validates its request against the schema, builds the typed request, and calls the op. An integration test asserts that the same operation invoked via library, CLI, and MCP produces identical results.
