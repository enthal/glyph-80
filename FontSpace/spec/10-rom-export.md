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

A config must not permanently depend on an in-memory document ID; the workspace binds a config to an open glyph set at runtime (chapter 11).

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
pub fn evaluate_output_word(source: &GlyphSet, config: &ExportConfig, address: u64)
    -> Result<u64, ExportError>;
```

For each output address: decode address bits into logical coordinates (code, page, addressed x/y); select the page and the glyph for that code (blank if absent); evaluate each output bit against the glyph; assemble the word; place it into the logical memory image. File encoding is a separate, later stage (§10.9).

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
