//! Export configuration (spec/10): a named, persisted specification for producing a
//! ROM/programmer image from a glyph set by addressing it **by character `code`**.
//!
//! **Milestone 5, strict-1:1 subset.** This carries the fields the v1 1:1 export needs
//! — the source binding, the address map, the data map, and the output format. The
//! geometry pipeline of spec/10 §10.8 (`transforms`, `packing`, `memory_image`) is not
//! yet represented; those fields extend this struct later via serde defaults (no
//! migration). The evaluation, the 1:1 coverage validator, and the encoders live in the
//! `fontspace-export` crate; this module is only the persisted schema.

use crate::{ExportComponentId, ExportConfigId, GlyphSetId, PageId};

/// A named specification for producing a ROM/programmer image from a glyph set
/// (spec/10 §10.1). Persisted in a FontSpace document; bound to an open glyph set by
/// its stable [`GlyphSetId`] (never a runtime document id — spec/10 §10.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportConfig {
    pub id: ExportConfigId,
    pub name: String,
    pub description: String,
    /// Which glyph set and page sequence the ROM is generated from.
    pub source: ExportSourceSpec,
    /// The meaning of each ROM **address** bit (spec/10 §10.4).
    pub address_map: AddressMap,
    /// What each ROM **output** (data) bit emits (spec/10 §10.5).
    pub data_map: DataMap,
    /// The programmer-file encoding (spec/10 §10.9). v1: `RawBinary`.
    pub output_format: OutputFormatConfig,
}

/// The source of an export: one glyph set and the ordered pages that the address map's
/// [`AddressBitSource::PageBit`]s index (page `n` = `pages[n]`). Referencing the glyph
/// set by its persisted [`GlyphSetId`] keeps the config file-stable (spec/10 §10.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportSourceSpec {
    pub glyph_set_id: GlyphSetId,
    pub pages: Vec<PageId>,
}

/// Defines the meaning of each ROM address line: `address_bits[i]` describes `Ai`
/// (spec/10 §10.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddressMap {
    pub id: ExportComponentId,
    pub name: String,
    pub address_bits: Vec<AddressBitSource>,
}

/// What drives one ROM address line (spec/10 §10.4). `CodeBit` — not an ordinal-slot
/// bit — is what makes the ROM directly character-addressable (spec/10 §10.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddressBitSource {
    /// A fixed 0/1 line.
    Constant(bool),
    /// Bit `n` of the character `code` (the glyph-selection dimension).
    CodeBit(u8),
    /// Bit `n` of the page index into [`ExportSourceSpec::pages`].
    PageBit(u8),
    /// Bit `n` of the addressed pixel column.
    PixelXBit(u8),
    /// Bit `n` of the addressed pixel row.
    PixelYBit(u8),
    /// The inverse of the wrapped line.
    Inverted(Box<AddressBitSource>),
}

/// Defines what each ROM output (data) bit emits: `output_bits[i]` describes `Di`
/// (spec/10 §10.5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataMap {
    pub id: ExportComponentId,
    pub name: String,
    pub output_bits: Vec<OutputBitSource>,
}

/// What one ROM data bit emits (spec/10 §10.5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputBitSource {
    /// A fixed 0/1 bit.
    Constant(bool),
    /// The value of a glyph pixel, its coordinates resolved against the addressed
    /// row/column (§10.5). An out-of-glyph coordinate reads off (blank).
    Pixel {
        x: CoordinateExpr,
        y: CoordinateExpr,
    },
    /// The inverse of the wrapped bit.
    Inverted(Box<OutputBitSource>),
}

/// A pixel-coordinate expression for a data bit (spec/10 §10.5), resolved per address
/// from the addressed pixel position decoded out of the address map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoordinateExpr {
    Constant(i32),
    AddressedX,
    AddressedY,
    AddressedXPlus(i32),
    AddressedYPlus(i32),
}

/// The programmer-file encoding (spec/10 §10.9). v1 implements only `RawBinary`; other
/// formats persist as an `Unsupported` placeholder so the schema is stable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputFormatConfig {
    /// A dense little-endian raw image (v1). Words up to 8 bits emit one byte each.
    RawBinary,
    /// A named format not yet implemented (Intel HEX, Motorola S-record, …).
    Unsupported { name: String },
}
