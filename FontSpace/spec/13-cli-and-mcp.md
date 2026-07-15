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

fontspace render-text file.fontspace.json \
  --glyph-set "Terminal 8x16" --text-nl "Hello\nWorld"

fontspace extract file.fontspace.json \
  --glyph-set "Terminal 8x16" --page Regular \
  --glyphs A-Z --output fragment.json

fontspace paste file.fontspace.json \
  --fragment fragment.json \
  --glyph-set "Terminal 8x16" --page Bold \
  --mapping by-code
```

Glyph ranges accept characters (`A-Z`), decimal, or `0x` hex codes, resolved via the referenced character set.

**`extract`** copies the selected glyphs off **one** page (`--page` must resolve to exactly one page — an ambiguous or multi-page selector is an error, never silently narrowed) into a canonical glyph fragment (§8.5). `--glyphs` defaults to all glyphs. Because a glyph fragment is a flat, page-agnostic list of codes, copying whole pages or several pages at once is the job of a `Pages` fragment, which arrives with that slice.

**`paste`** places a glyph fragment onto one page (again `--page` must resolve to exactly one), returning one invertible change. `--mapping` chooses the destination code — `by-code` (the default) or `sequential-from-code:CODE` (§8.3). `--size` chooses the geometry policy: `require-exact` (the default — a size difference is an error, never a silent resize), `center`, or `place-at:X,Y` (a signed pixel offset); the placement conversions clip whatever falls outside and never resample. Like every mutating command it honors `--dry-run` and writes atomically. `by-slot`, `crop`, and `scale-nearest` land with later slices.

**`render-text` subjects.** The subject is exactly one of three mutually-exclusive forms; supplying more than one is an error:

- `--glyphs <selector>` — a code selector as above. **Omitting it renders all glyphs** (the character set's full entry order).
- `--text <string>` — render the string as one line. Each input character maps to an 8-bit `code` (its Unicode scalar, equal to the byte for Latin-1 input); repeats are fine. Characters with no character-set entry are **ignored** (§9.2.1); a defined-but-blank character (e.g. a space) still occupies its cell.
- `--text-nl <string>` — like `--text`, but a newline codepoint starts a new line rather than mapping to a glyph.

Requirements:

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
