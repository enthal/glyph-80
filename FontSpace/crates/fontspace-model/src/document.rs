//! The `FontSpace` document aggregate (spec/03 §3.1).
//!
//! One document is one `.fontspace.json` file: a container of named, stable
//! objects. There is **no** global glyph size and **no** global character set at
//! the document level — a document may hold any combination of character sets,
//! glyph sets, and export configs.

use crate::{CharacterSet, ExportConfig, FontSpaceId, GlyphSet, IdGen};

/// The document schema version this build reads and writes. Persistence and
/// migration (spec/06) dispatch on this; document validation rejects other values.
pub const CURRENT_FORMAT_VERSION: u32 = 1;

/// Free-text document metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontSpaceMetadata {
    pub name: String,
    pub description: String,
}

/// A FontSpace document: character sets, glyph sets, and export configs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontSpace {
    pub format_version: u32,
    pub id: FontSpaceId,
    pub metadata: FontSpaceMetadata,
    pub character_sets: Vec<CharacterSet>,
    pub glyph_sets: Vec<GlyphSet>,
    pub export_configs: Vec<ExportConfig>,
}

impl FontSpace {
    /// A new, empty document at [`CURRENT_FORMAT_VERSION`] with a freshly-minted id.
    pub fn new(
        ids: &mut dyn IdGen,
        name: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        Self {
            format_version: CURRENT_FORMAT_VERSION,
            id: FontSpaceId::new(ids),
            metadata: FontSpaceMetadata {
                name: name.into(),
                description: description.into(),
            },
            character_sets: Vec::new(),
            glyph_sets: Vec::new(),
            export_configs: Vec::new(),
        }
    }

    /// The character set with `id`, if present.
    pub fn character_set(&self, id: crate::CharacterSetId) -> Option<&CharacterSet> {
        self.character_sets.iter().find(|cs| cs.id == id)
    }

    /// The glyph set with `id`, if present.
    pub fn glyph_set(&self, id: crate::GlyphSetId) -> Option<&GlyphSet> {
        self.glyph_sets.iter().find(|gs| gs.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SequentialIdGen;

    #[test]
    fn new_document_is_empty_and_current_version() {
        let mut ids = SequentialIdGen::new();
        let doc = FontSpace::new(&mut ids, "My Font", "desc");
        assert_eq!(doc.format_version, CURRENT_FORMAT_VERSION);
        assert_eq!(doc.metadata.name, "My Font");
        assert!(doc.character_sets.is_empty());
        assert!(doc.glyph_sets.is_empty());
        assert!(doc.export_configs.is_empty());
    }
}
