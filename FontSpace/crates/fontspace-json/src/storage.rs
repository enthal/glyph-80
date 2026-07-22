//! The persisted storage schema (spec/06 §6.2): explicit `Stored*` structs, distinct
//! from the runtime domain types. Field declaration order **is** the on-disk field
//! order, so it must match the canonical example in spec/06 §6.5. Loading
//! deserializes into these, then converts+validates into domain types; saving does
//! the reverse. This localizes the wire format and future migration.

use serde::{Deserialize, Serialize};

/// A version-tagged peek used to dispatch migration before full deserialization.
#[derive(Deserialize)]
pub(crate) struct VersionPeek {
    pub format_version: u32,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct StoredFontSpaceV1 {
    pub format_version: u32,
    pub id: String,
    pub metadata: StoredMetadata,
    #[serde(default)]
    pub character_sets: Vec<StoredCharacterSet>,
    #[serde(default)]
    pub glyph_sets: Vec<StoredGlyphSet>,
    #[serde(default)]
    pub export_configs: Vec<StoredExportConfig>,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct StoredMetadata {
    pub name: String,
    pub description: String,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct StoredCharacterSet {
    pub id: String,
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub entries: Vec<StoredCharacterEntry>,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct StoredCharacterEntry {
    pub code: String,
    pub label: String,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct StoredGlyphSet {
    pub id: String,
    pub name: String,
    pub description: String,
    pub glyph_size: StoredGlyphSize,
    pub character_set_id: String,
    #[serde(default)]
    pub pages: Vec<StoredGlyphPage>,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct StoredGlyphSize {
    pub width: u16,
    pub height: u16,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct StoredGlyphPage {
    pub id: String,
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub guides: Vec<StoredGuide>,
    #[serde(default)]
    pub glyphs: Vec<StoredGlyph>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum StoredAxis {
    Horizontal,
    Vertical,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct StoredGuide {
    pub id: String,
    pub name: String,
    pub axis: StoredAxis,
    pub position: i32,
    pub visible: bool,
    pub locked: bool,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct StoredGlyph {
    pub code: String,
    pub pixels: Vec<String>,
}

/// The persisted export config (spec/10 §10.1), strict-1:1 subset (Milestone 5): the
/// source binding, address/data maps, and output format. Field order is on-disk order.
#[derive(Serialize, Deserialize)]
pub(crate) struct StoredExportConfig {
    pub id: String,
    pub name: String,
    pub description: String,
    pub source: StoredExportSource,
    pub address_map: StoredAddressMap,
    pub data_map: StoredDataMap,
    pub output_format: StoredOutputFormat,
    /// Target size as a power-of-two exponent (address bits), or absent/`null` for the
    /// natural image size (spec/10 §10.9).
    #[serde(default)]
    pub output_address_bits: Option<u8>,
    /// Padding/hole byte; defaults to the erased-EEPROM `0xFF` for documents predating
    /// this field.
    #[serde(default = "default_fill_byte")]
    pub fill_byte: u8,
}

fn default_fill_byte() -> u8 {
    fontspace_model::DEFAULT_FILL_BYTE
}

#[derive(Serialize, Deserialize)]
pub(crate) struct StoredExportSource {
    pub glyph_set_id: String,
    #[serde(default)]
    pub pages: Vec<String>,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct StoredAddressMap {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub address_bits: Vec<StoredAddressBit>,
}

/// One address line's source (spec/10 §10.4). Externally tagged, snake_case, e.g.
/// `{"code": 5}`, `{"pixel_y": 0}`, `{"inverted": {"code": 3}}`.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum StoredAddressBit {
    Constant(bool),
    Code(u8),
    Page(u8),
    PixelX(u8),
    PixelY(u8),
    Inverted(Box<StoredAddressBit>),
}

#[derive(Serialize, Deserialize)]
pub(crate) struct StoredDataMap {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub output_bits: Vec<StoredOutputBit>,
}

/// One data bit's source (spec/10 §10.5), e.g. `{"pixel": {"x": "addressed_x", "y":
/// "addressed_y"}}`, `{"constant": false}`, `{"inverted": {...}}`.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum StoredOutputBit {
    Constant(bool),
    Pixel {
        x: StoredCoordExpr,
        y: StoredCoordExpr,
    },
    Inverted(Box<StoredOutputBit>),
}

/// A pixel-coordinate expression (spec/10 §10.5): unit variants as strings
/// (`"addressed_x"`), data variants tagged (`{"constant": 3}`, `{"addressed_x_plus": 1}`).
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum StoredCoordExpr {
    Constant(i32),
    AddressedX,
    AddressedY,
    AddressedXPlus(i32),
    AddressedYPlus(i32),
}

/// The programmer-file encoding (spec/10 §10.9): `"raw_binary"` or `{"unsupported":
/// {"name": "…"}}`.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum StoredOutputFormat {
    RawBinary,
    Unsupported { name: String },
}
