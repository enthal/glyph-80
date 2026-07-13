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

/// Provisional (spec/10 lands at Milestone 5); mirrors the model stub.
#[derive(Serialize, Deserialize)]
pub(crate) struct StoredExportConfig {
    pub id: String,
    pub name: String,
    pub description: String,
}
