# 6. JSON Persistence

## 6.1 General rules

The canonical file format is pretty-printed JSON via `serde` / `serde_json`. Files use the extension `.fontspace.json`.

The writer must be **deterministic**:

- stable field order, from explicit storage structs (not runtime maps);
- stable array order (glyphs in charset-entry order; everything else in its domain order);
- no generated timestamps on save;
- no hash-map iteration order in persisted content;
- fixed indentation (2 spaces);
- exactly one final newline;
- lowercase UUID strings.

The required determinism property (chapter 15): `save(load(save(d))) == save(d)`, byte-for-byte.

## 6.2 Storage structs vs domain types

Persisted JSON uses explicit storage-schema structs (`StoredFontSpaceV1`, `StoredGlyphSet`, …), never runtime or GUI structs. Loading deserializes into storage structs, validates, and converts to domain types; saving converts domain types to storage structs. This decouples the wire format from in-memory representation and localizes migration.

```rust
#[derive(Serialize, Deserialize)]
pub struct StoredFontSpaceV1 {
    pub format_version: u32,
    pub id: String,
    pub metadata: StoredMetadata,
    #[serde(default)] pub character_sets: Vec<StoredCharacterSet>,
    #[serde(default)] pub glyph_sets: Vec<StoredGlyphSet>,
    #[serde(default)] pub export_configs: Vec<StoredExportConfig>,
}
```

`#[serde(default)]` lets a document contain only some object kinds (chapter 1).

## 6.3 Character entries

An entry stores its `code` (as a hex string for readability) and `label`:

```json
{ "code": "0x41", "label": "LATIN CAPITAL LETTER A" }
```

`code` is written as a lowercase-`0x` hex string and parsed leniently (decimal or `0x` hex accepted on load; always written as `0x` hex). Entry order in the array is the canonical ordinal order.

## 6.4 Glyphs as visual rows

Glyphs store their `code` and their bitmap as an array of visual row strings, for high-quality diffs:

```json
{
  "code": "0x41",
  "pixels": [
    "..###...",
    ".#...#..",
    ".#...#..",
    ".#####..",
    ".#...#..",
    ".#...#..",
    ".#...#..",
    "........"
  ]
}
```

Canonical pixel characters: `.` = off, `#` = on. Loader requirements:

- row count must equal the glyph set's `glyph_size.height`;
- each row's Unicode scalar count must equal `glyph_size.width`;
- only `.` and `#` are accepted in canonical files;
- malformed data produces a precise error identifying glyph set, page, code, row, and column where possible.

Explicit import/paste may accept alternate on/off characters, but **saving always normalizes to `.` and `#`**. A page's glyph array is written **in charset-entry order** (chapter 4), and **blank glyphs are omitted** (chapter 5 §5.6). A **dangling** glyph — one whose `code` has no entry in the referenced character set (chapter 4 §4.4), and so has no ordinal — is written **after** all entry-ordered glyphs, in **ascending `code` order**, so output stays deterministic even in the tolerated dangling case.

## 6.5 Example document

```json
{
  "format_version": 1,
  "id": "8b15e874-52ec-4a73-b915-55a889d1a4da",
  "metadata": {
    "name": "Terminal Experiments",
    "description": "ASCII and hardware display font work"
  },
  "character_sets": [
    {
      "id": "6b991465-c855-4b51-b56a-65e86d3388a0",
      "name": "ASCII 7-bit",
      "description": "Codes 0x00 through 0x7F",
      "entries": [
        { "code": "0x00", "label": "NUL" },
        { "code": "0x41", "label": "LATIN CAPITAL LETTER A" }
      ]
    }
  ],
  "glyph_sets": [
    {
      "id": "44d99889-9d09-4d89-b29e-18cb726ce728",
      "name": "Terminal 8x16",
      "description": "Main terminal font",
      "glyph_size": { "width": 8, "height": 16 },
      "character_set_id": "6b991465-c855-4b51-b56a-65e86d3388a0",
      "pages": [
        {
          "id": "1fa1cab5-b530-493e-9937-31ddc214a88c",
          "name": "Regular",
          "description": "Primary page",
          "guides": [
            {
              "id": "94d3bc61-540b-4e85-aed7-ea7f978d4ca6",
              "name": "Baseline",
              "axis": "horizontal",
              "position": 13,
              "visible": true,
              "locked": false
            }
          ],
          "glyphs": [
            {
              "code": "0x41",
              "pixels": [
                "..####..", ".#....#.", ".#....#.", ".#....#.",
                ".######.", ".#....#.", ".#....#.", ".#....#.",
                ".#....#.", ".#....#.", ".#....#.", ".#....#.",
                "........", "........", "........", "........"
              ]
            }
          ]
        }
      ]
    }
  ],
  "export_configs": []
}
```

Note the `Regular` page defines a glyph only for `0x41`; `0x00` (`NUL`) renders blank and is not stored.

## 6.6 Versioning and migration

`format_version` is required. The loader dispatches explicitly by version and migrates older schemas into the current domain model. Do not silently reinterpret incompatible semantics. An unknown future version produces a clear unsupported-version error. Every migration path is unit-tested with a checked-in golden input at the old version and expected output at the current version.
