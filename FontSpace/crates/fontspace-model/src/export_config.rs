//! Export configuration (spec/10).
//!
//! **Provisional at Milestone 1.** The full schema — address/data maps, components,
//! transforms, and the strict 1:1 constraint — is specified in
//! `FontSpace/spec/10-rom-export.md` and implemented in the `fontspace-export`
//! crate at Milestone 5. This stub carries only identity and naming so that the
//! `FontSpace` document aggregate matches spec/03 §3.1 today and document
//! validation can enforce export-config id uniqueness. It will grow in place.

use crate::{ExportConfigId, IdGen};

/// A named specification for producing a ROM/programmer image from a glyph set.
/// Fields beyond identity/naming arrive with Milestone 5 (spec/10).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportConfig {
    pub id: ExportConfigId,
    pub name: String,
    pub description: String,
}

impl ExportConfig {
    /// A new export config with a freshly-minted id. Provisional (see module docs).
    pub fn new(
        ids: &mut dyn IdGen,
        name: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        Self {
            id: ExportConfigId::new(ids),
            name: name.into(),
            description: description.into(),
        }
    }
}
