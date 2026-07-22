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

fontspace add-export-config file.fontspace.json \
  --name "AT28C64 Text ROM" --glyph-set "Terminal 8x16" \
  --pages Regular,Bold,Italic,Underline --code-bits 7

fontspace validate-export file.fontspace.json --config "AT28C64 Text ROM"

fontspace export file.fontspace.json \
  --config "AT28C64 Text ROM" --output text-rom.bin
```

Glyph ranges accept characters (`A-Z`), decimal, or `0x` hex codes, resolved via the referenced character set.

**`extract`** copies the selected glyphs off **one** page (`--page` must resolve to exactly one page — an ambiguous or multi-page selector is an error, never silently narrowed) into a canonical glyph fragment (§8.5). `--glyphs` defaults to all glyphs. Because a glyph fragment is a flat, page-agnostic list of codes, copying whole pages or several pages at once is the job of a `Pages` fragment, which arrives with that slice.

**`paste`** places a glyph fragment onto one page (again `--page` must resolve to exactly one), returning one invertible change. `--mapping` chooses the destination code — `by-code` (the default), `by-slot` (place each glyph on the destination entry at the same ordinal), or `sequential-from-code:CODE` (§8.3). `--size` chooses the geometry policy: `require-exact` (the default — a size difference is an error, never a silent resize), `center`, `place-at:X,Y` (a signed pixel offset), or `scale-nearest` (nearest-neighbor resample); the placement conversions clip whatever falls outside, and only `scale-nearest` resamples. Like every mutating command it honors `--dry-run` and writes atomically. `crop` lands with a later slice.

**ROM export** (chapter 10). **`add-export-config`** adds a standard `--scan` (`row`, the default text-ROM layout, or `column`) 1:1 config to the document: the addressed pixel axis plus `--code-bits` code bits and page bits (sized to `--pages`, an ordered comma list of page names or `all`) go in the address, and the other axis comes out on the data bits (row-scan → column `x=0` is the most-significant data bit). `--output-address-bits N` pads the image to `2^N` bytes (N = the target EEPROM's address-bit count, so `16` → 64 KiB) and `--fill-byte` (default `0xFF`, `0x`-hex accepted) sets the padding byte (§10.9); both persist on the config. **`validate-export`** checks a named config is a strict 1:1 mapping (§10.7) and prints its shape, or a diagnostic. **`export`** validates and then writes the dense raw-binary image (§10.9) to `--output`; it never emits bytes from a non-1:1 config, and `--dry-run` prints the summary without writing (and so needs no `--output`). The `.bin` is an output artifact, not a document, so it is written directly (not the atomic document replacement).

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
