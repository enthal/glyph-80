# 10. ROM and Programmer Export

## 10.1 Export configs are persistent objects

Export settings are named, saved, copyable entities that live in a FontSpace file (the same file as their glyph set, or a different one — the user decides).

```rust
pub struct ExportConfig {
    pub id: ExportConfigId,
    pub name: String,
    pub description: String,
    pub source: ExportSourceSpec,
    pub transforms: Vec<GlyphTransform>,   // v1: must be empty
    pub packing: PackingConfig,
    pub address_map: AddressMap,
    pub data_map: DataMap,
    pub memory_image: MemoryImageConfig,
    pub output_format: OutputFormatConfig,
}
```

A config must not permanently depend on an in-memory document ID; the workspace binds a config to an open glyph set at runtime (chapter 11). It **is** file-stable through the persisted `GlyphSetId` and page ids in `source` (spec/10 §10.4), never a runtime `DocumentId`.

**Implemented subset (Milestone 5, v1).** The persisted type carries the four strict-1:1 fields — `source`, `address_map`, `data_map`, and `output_format` — plus identity/naming. The geometry-pipeline fields (`transforms`, `packing`, `memory_image` — §10.8) are **not yet represented**; they extend the struct in a later slice through serde defaults (no migration, since a config without them is a valid 1:1 config). Until `transforms` exists, the §10.3 "`transforms.is_empty()`" constraint is satisfied trivially.

**Wire form** (canonical JSON, spec/06). `source` is `{glyph_set_id, pages: [page-id, …]}` — page `n` in an `AddressBitSource::PageBit(n)` indexes `pages`. Each address/data bit is an externally-tagged, snake_case object: `{"pixel_y": 0}`, `{"code": 5}`, `{"page": 0}`, `{"constant": true}`, `{"inverted": {…}}` for address lines; `{"pixel": {"x": …, "y": …}}`, `{"constant": false}`, `{"inverted": {…}}` for data bits, where a coordinate is a unit string (`"addressed_x"`, `"addressed_y"`) or a tagged object (`{"constant": 3}`, `{"addressed_x_plus": 1}`, `{"addressed_y_plus": -1}`). `output_format` is `"raw_binary"` or `{"unsupported": {"name": "…"}}`. See `crates/fontspace-json/testdata/export.fontspace.json` for a full canonical example.

## 10.2 The addressing model: address by `code`

The Glyph-80 display hardware addresses the font ROM by **character code**: the byte in display RAM, plus a row/column counter, selects the ROM word that drives the pixels. FontSpace's export model matches this exactly. The ROM's glyph dimension is indexed by entry **`code`** (chapter 4), not by ordinal position. Consequences:

- The ROM spans the full addressable **code range** implied by the address map's code bits (e.g. 7 code bits → codes `0x00`–`0x7F`).
- A code with no glyph in the selected page — whether the character set omits it or the page simply doesn't define it — emits a **blank** (all-off) word. The ROM is dense over the code range regardless of how sparse the page is.
- Ordinal position is irrelevant to the ROM. It matters only for display and canonical storage order.

This is why `code` is the entry's identity: it is the thing the hardware addresses, so it is the thing the export addresses.

## 10.3 Initial-release constraint: strict 1:1

v1 implements only strict 1:1 export:

- exactly one source glyph set;
- `transforms.is_empty()` (no geometric transforms);
- one selected page sequence;
- each logical glyph pixel maps exactly once to one ROM output bit at one address — no duplicated or omitted pixel bits;
- glyph width compatible with the ROM data width;
- the address map covers exactly the required code, row/column, and page dimensions — no gaps, no overlaps;
- output image is dense.

The validator (chapter 14) explains any incompatibility. Example success summary:

```text
Valid 1:1 export
8 glyph pixels per row
8 ROM data bits
16 rows per glyph
128 codes (7 code bits)
4 pages
8192 output bytes
```

## 10.4 Address map

An address map defines the meaning of each ROM address bit.

```rust
pub struct AddressMap { pub id: ExportComponentId, pub name: String, pub address_bits: Vec<AddressBitSource> }

pub enum AddressBitSource {
    Constant(bool),
    CodeBit(u8),        // bit n of the character code  (the glyph-selection dimension)
    PageBit(u8),
    PixelXBit(u8),
    PixelYBit(u8),
    Inverted(Box<AddressBitSource>),
}
```

`address_bits[i]` describes address line `Ai`. Example for an 8×16, 128-code, 4-page text ROM (13 address bits, 8 KiB):

```text
A0..A3  = pixel_y bit 0..3     (row within glyph, 16 rows)
A4..A10 = code bit 0..6        (character code, 128 codes)
A11..A12 = page bit 0..1       (4 pages)
```

Here the row is in the address and the 8 pixels of that row come out on the data bits (§10.5). `CodeBit` — not an ordinal-slot bit — is what makes the ROM directly character-addressable. v1 avoids a general expression language.

**Field layout.** The three address dimensions — the scanned **pixel** axis, the **code**, and the **page** — are laid out as contiguous fields in any order (low→high), each optionally **bit-reversed** (its bits run most-significant-first within its span of address lines), to match hardware whose address lines aren't wired in the default order. `fontspace-export::build_scan_config` builds this from a field order + per-field reverse flags + a data-bit-order flag, and `layout_of` reads it back; the scan presets are `build_scan_config` with the default order (`[pixel, code, page]`, none reversed) and MSB-first data. Reordering or reversing a field is a permutation of the same address space, so the 1:1 coverage (§10.7) is unaffected — the validator accepts any such layout.

## 10.5 Data map

A data map defines what each ROM output bit emits.

```rust
pub struct DataMap { pub id: ExportComponentId, pub name: String, pub output_bits: Vec<OutputBitSource> }

pub enum OutputBitSource {
    Constant(bool),
    Pixel { x: CoordinateExpr, y: CoordinateExpr },
    Inverted(Box<OutputBitSource>),
}

pub enum CoordinateExpr {
    Constant(i32),
    AddressedX, AddressedY,
    AddressedXPlus(i32), AddressedYPlus(i32),
}
```

Example (data bit `Dn` emits pixel `x=7-n` of the addressed row):

```text
D7 = pixel x=0 at addressed row
D6 = pixel x=1 at addressed row
...
D0 = pixel x=7 at addressed row
```

## 10.6 Logical evaluation

```rust
pub fn evaluate_output_word(source: &GlyphSet, config: &ExportConfig, address: u64) -> u64;
```

For each output address: decode address bits into logical coordinates (code, page, addressed x/y); select the page and the glyph for that code (blank if absent); evaluate each output bit against the glyph; assemble the word (bit `i` = `data_map.output_bits[i]`, i.e. `Di`); place it into the logical memory image. Evaluation is **infallible** — a missing page, a missing glyph, or an out-of-glyph pixel simply reads off — so all failure is caught up front by the validator (§10.7), not per-address. `generate_image(source, config, limits)` produces the dense image (one word per address over `2^address_bits`) and is fallible only on the size/width limits (chapter 16). File encoding is a separate, later stage (§10.9).

## 10.7 Coverage validation

1:1 validity is a real algorithm, not a flag. The validator checks:

- exactly one source glyph set; `transforms.is_empty()`;
- the address bits partition the required dimensions — every `CodeBit`, `PageBit`, and the pixel row/column bits present exactly the count needed to address the selected pages and geometry, with **no dimension appearing in both the address map and the data map** (the general types allow it; 1:1 forbids it, since that would double-count or under-cover a coordinate);
- data-map width equals the source glyph width (for row-scan) or height (for column-scan);
- every selected code is addressable within the code bits;
- computed image size is within the configured limit (chapter 16).

Example diagnostic:

```text
Export config "AT28C64 Text ROM" cannot be rendered:
glyph width is 16 but the data map has 8 output bits.
Add a split or packing transform when that feature is available.
```

**Implemented (Milestone 5, `fontspace-export::validate_export`).** v1 validates the row-scan and column-scan strict-1:1 shapes: every address line must be a plain dimension bit (no `Constant`/`Inverted` in the address); **exactly one** pixel axis is addressed (`pixel_y` → row-scan, `pixel_x` → column-scan) and the other axis is emitted by the data bits, so no pixel dimension appears in both maps; each dimension's bits must partition it (contiguous `0..N`, no gap or duplicate) and cover the glyph/page/code extents; and the data map must be exactly one glyph pixel per data bit at the addressed line, together a permutation of `0..width` (row-scan) or `0..height` (column-scan). A `validate_export` success returns an [`ExportSummary`] whose `Display` is the summary above. The general `AddressBitSource`/`OutputBitSource` types still permit richer maps; those are simply not 1:1 and are rejected with a reason.

## 10.8 Future geometry pipeline (not v1)

The architecture leaves room for:

```text
source glyph -> geometry transforms -> logical export units -> packing/order
             -> address/data mapping -> logical memory image -> programmer file encoding
```

```rust
pub enum GlyphTransform { Pad(..), Crop(..), Split(..), Tile(..), Rotate(..), Flip(..), RemapCoordinates(..) }
```

`transforms` is persisted now (so the schema is stable) but v1 requires it empty and the validator rejects non-empty. Splitting produces multiple `LogicalUnit`s per source glyph; the packing stage (`PackingConfig` with `UnitOrderKey`/`ScanOrder`) decides output order and banking. These types may be stubbed in v1 with a single trivial packing that preserves 1:1.

## 10.9 Output encoders

Distinguish the **logical memory image** from the **programmer file**:

```rust
pub enum OutputFormatConfig {
    RawBinary(RawBinaryConfig),
    IntelHex(IntelHexConfig),
    MotorolaSRecord(MotorolaSRecordConfig),
    UnsupportedPlaceholder { name: String },
}
```

v1 implements only `RawBinary`. The logical image (an addressed array of words) is produced by `fontspace-export` and is what golden tests assert against; the encoder is a thin final stage added per format.

**Output size and fill.** A config carries an optional `output_address_bits` and a `fill_byte` (default `0xFF`, the erased-EEPROM value). `output_address_bits` is the target EEPROM's **address-bit count**, so the size is `2^output_address_bits` **bytes** (e.g. `16` → 64 KiB, `20` → 1 MiB) — a power of two by construction, no "is it a power of two?" check needed. When set it must be no smaller than the natural image and within the size limit; the encoder pads the image up to it with `fill_byte`, so a small ROM fills a larger EEPROM (minipro and friends). `None` means the natural size. `render_rom` is the one entry point that validates, generates, encodes, and pads — it never emits bytes from a config that isn't a valid strict-1:1 mapping. (Once the memory-mapped multi-bank model lands, `fill_byte` also fills any address not covered by a bank.)
