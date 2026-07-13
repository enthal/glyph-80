//! Document validation (spec/14 §14.1).
//!
//! Structural validation of a whole [`FontSpace`] document: stable-id uniqueness,
//! character-set references, entry-code uniqueness, geometry agreement, the
//! referential-integrity / per-page uniqueness rules (spec/04 §4.4), and the
//! resource limits (spec/16). Hard problems are [`ValidationError`]s; a glyph whose
//! `code` has no entry is a **tolerated** [`ValidationWarning::DanglingGlyph`], not
//! an error (spec/04 §4.4). Errors carry object context (spec/14 §14.4).
//!
//! Iteration is over the document's vectors in order and membership uses hash sets,
//! so the report is deterministic (no hash-map iteration reaches the output).

use std::collections::{HashMap, HashSet};

use crate::{
    CharacterSetId, ExportConfigId, FontSpace, GlyphSetId, GlyphSize, GlyphSizeError, GuideId,
    Limits, PageId, document::CURRENT_FORMAT_VERSION,
};

/// The outcome of validating a document: hard errors and tolerated warnings.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ValidationReport {
    pub errors: Vec<ValidationError>,
    pub warnings: Vec<ValidationWarning>,
}

impl ValidationReport {
    /// True when there are no hard errors. Warnings do not make a document invalid.
    pub fn is_valid(&self) -> bool {
        self.errors.is_empty()
    }
}

/// A hard document-validation failure (spec/14 §14.1).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ValidationError {
    #[error("unsupported document format_version {found}; this build supports {supported}")]
    UnsupportedFormatVersion { found: u32, supported: u32 },
    #[error("duplicate character set id {0:?}")]
    DuplicateCharacterSetId(CharacterSetId),
    #[error("duplicate glyph set id {0:?}")]
    DuplicateGlyphSetId(GlyphSetId),
    #[error("duplicate page id {0:?}")]
    DuplicatePageId(PageId),
    #[error("duplicate guide id {0:?}")]
    DuplicateGuideId(GuideId),
    #[error("duplicate export config id {0:?}")]
    DuplicateExportConfigId(ExportConfigId),
    #[error("glyph set {glyph_set:?} references unknown character set {character_set:?}")]
    UnknownCharacterSet {
        glyph_set: GlyphSetId,
        character_set: CharacterSetId,
    },
    #[error("character set {character_set:?}: duplicate entry code {code:#06x}")]
    DuplicateEntryCode {
        character_set: CharacterSetId,
        code: u32,
    },
    #[error("glyph set {glyph_set:?}: invalid glyph size ({source})")]
    GlyphSizeInvalid {
        glyph_set: GlyphSetId,
        source: GlyphSizeError,
    },
    #[error(
        "glyph set {glyph_set:?} / page {page:?} / code {code:#06x}: bitmap is {found}, expected {expected}"
    )]
    GlyphSizeMismatch {
        glyph_set: GlyphSetId,
        page: PageId,
        code: u32,
        expected: GlyphSize,
        found: GlyphSize,
    },
    #[error("glyph set {glyph_set:?} / page {page:?}: duplicate glyph for code {code:#06x}")]
    DuplicateGlyphCode {
        glyph_set: GlyphSetId,
        page: PageId,
        code: u32,
    },
    #[error("character set {character_set:?} has {count} entries, exceeding the limit of {max}")]
    TooManyEntries {
        character_set: CharacterSetId,
        count: usize,
        max: u32,
    },
    #[error("glyph set {glyph_set:?} has {count} pages, exceeding the limit of {max}")]
    TooManyPages {
        glyph_set: GlyphSetId,
        count: usize,
        max: u32,
    },
    #[error("document has {count} total glyph pixels, exceeding the limit of {max}")]
    TotalGlyphPixelsExceeded { count: u64, max: u64 },
}

/// A tolerated, non-fatal validation observation (spec/14 §14.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationWarning {
    /// A glyph whose `code` has no entry in the referenced character set. Tolerated:
    /// loaded, warned, and treated as blank/unaddressable until resolved
    /// (spec/04 §4.4).
    DanglingGlyph {
        glyph_set: GlyphSetId,
        page: PageId,
        code: u32,
    },
}

impl std::fmt::Display for ValidationWarning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ValidationWarning::DanglingGlyph {
                glyph_set,
                page,
                code,
            } => write!(
                f,
                "dangling glyph: glyph set {glyph_set:?} / page {page:?} / code {code:#06x} has no character-set entry"
            ),
        }
    }
}

impl FontSpace {
    /// Validate the whole document against the structural invariants and `limits`.
    /// Never mutates; collects every problem rather than failing on the first.
    pub fn validate(&self, limits: &Limits) -> ValidationReport {
        let mut report = ValidationReport::default();

        if self.format_version != CURRENT_FORMAT_VERSION {
            report
                .errors
                .push(ValidationError::UnsupportedFormatVersion {
                    found: self.format_version,
                    supported: CURRENT_FORMAT_VERSION,
                });
        }

        // Character sets: id uniqueness, entry-code uniqueness, slot-count limit.
        // Build each set's code membership up front for the referential-integrity
        // check below.
        let mut seen_character_set_ids = HashSet::new();
        let mut entry_codes_by_character_set_id: HashMap<CharacterSetId, HashSet<u32>> =
            HashMap::new();
        for character_set in &self.character_sets {
            if !seen_character_set_ids.insert(character_set.id) {
                report
                    .errors
                    .push(ValidationError::DuplicateCharacterSetId(character_set.id));
            }
            if character_set.entries.len() as u64 > limits.max_character_slots_per_set as u64 {
                report.errors.push(ValidationError::TooManyEntries {
                    character_set: character_set.id,
                    count: character_set.entries.len(),
                    max: limits.max_character_slots_per_set,
                });
            }
            let mut codes = HashSet::new();
            for entry in &character_set.entries {
                if !codes.insert(entry.code) {
                    report.errors.push(ValidationError::DuplicateEntryCode {
                        character_set: character_set.id,
                        code: entry.code,
                    });
                }
            }
            entry_codes_by_character_set_id.insert(character_set.id, codes);
        }

        // Glyph sets, pages, glyphs: ids, references, geometry, uniqueness, dangling.
        let mut seen_glyph_set_ids = HashSet::new();
        let mut seen_page_ids = HashSet::new();
        let mut seen_guide_ids = HashSet::new();
        let mut total_glyph_pixels: u64 = 0;

        for glyph_set in &self.glyph_sets {
            if !seen_glyph_set_ids.insert(glyph_set.id) {
                report
                    .errors
                    .push(ValidationError::DuplicateGlyphSetId(glyph_set.id));
            }
            if let Err(source) = glyph_set.glyph_size.validate(limits) {
                report.errors.push(ValidationError::GlyphSizeInvalid {
                    glyph_set: glyph_set.id,
                    source,
                });
            }
            let known_codes = entry_codes_by_character_set_id.get(&glyph_set.character_set_id);
            if known_codes.is_none() {
                report.errors.push(ValidationError::UnknownCharacterSet {
                    glyph_set: glyph_set.id,
                    character_set: glyph_set.character_set_id,
                });
            }
            if glyph_set.pages.len() as u64 > limits.max_pages_per_glyph_set as u64 {
                report.errors.push(ValidationError::TooManyPages {
                    glyph_set: glyph_set.id,
                    count: glyph_set.pages.len(),
                    max: limits.max_pages_per_glyph_set,
                });
            }

            for page in &glyph_set.pages {
                if !seen_page_ids.insert(page.id) {
                    report
                        .errors
                        .push(ValidationError::DuplicatePageId(page.id));
                }
                for guide in &page.guides {
                    if !seen_guide_ids.insert(guide.id) {
                        report
                            .errors
                            .push(ValidationError::DuplicateGuideId(guide.id));
                    }
                }

                let mut seen_glyph_codes = HashSet::new();
                for glyph in &page.glyphs {
                    total_glyph_pixels =
                        total_glyph_pixels.saturating_add(glyph.bitmap.size().pixel_count());

                    if !seen_glyph_codes.insert(glyph.code) {
                        report.errors.push(ValidationError::DuplicateGlyphCode {
                            glyph_set: glyph_set.id,
                            page: page.id,
                            code: glyph.code,
                        });
                    }
                    if glyph.bitmap.size() != glyph_set.glyph_size {
                        report.errors.push(ValidationError::GlyphSizeMismatch {
                            glyph_set: glyph_set.id,
                            page: page.id,
                            code: glyph.code,
                            expected: glyph_set.glyph_size,
                            found: glyph.bitmap.size(),
                        });
                    }
                    // Referential integrity: a code absent from the referenced set is
                    // a tolerated dangling glyph (warning). Skip when the referenced
                    // set is unknown — that is already a hard error above.
                    if let Some(codes) = known_codes
                        && !codes.contains(&glyph.code)
                    {
                        report.warnings.push(ValidationWarning::DanglingGlyph {
                            glyph_set: glyph_set.id,
                            page: page.id,
                            code: glyph.code,
                        });
                    }
                }
            }
        }

        // Export configs: id uniqueness (provisional type; spec/10 lands at M5).
        let mut seen_export_config_ids = HashSet::new();
        for export_config in &self.export_configs {
            if !seen_export_config_ids.insert(export_config.id) {
                report
                    .errors
                    .push(ValidationError::DuplicateExportConfigId(export_config.id));
            }
        }

        if total_glyph_pixels > limits.max_total_glyph_pixels_per_document {
            report
                .errors
                .push(ValidationError::TotalGlyphPixelsExceeded {
                    count: total_glyph_pixels,
                    max: limits.max_total_glyph_pixels_per_document,
                });
        }

        report
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        Bitmap, CharacterEntry, CharacterSet, FontSpace, Glyph, GlyphPage, GlyphSet, GlyphSize,
        Guide, GuideAxis, Limits, SequentialIdGen,
    };

    use super::*;

    /// Builds a minimal valid document: one character set with codes 0x41/0x42, one
    /// 8×16 glyph set referencing it, one page with a matching-size glyph for 0x41.
    fn valid_doc() -> FontSpace {
        let mut ids = SequentialIdGen::new();
        let mut cs = CharacterSet::new(&mut ids, "ASCII-ish", "");
        cs.entries = vec![
            CharacterEntry {
                code: 0x41,
                label: "A".into(),
            },
            CharacterEntry {
                code: 0x42,
                label: "B".into(),
            },
        ];
        let size = GlyphSize::new(8, 16);
        let mut gs = GlyphSet::new(&mut ids, "Terminal 8x16", "", size, cs.id);
        let mut page = GlyphPage::new(&mut ids, "Regular", "");
        let mut bitmap = Bitmap::new_blank(size);
        bitmap.set(1, 1, true).unwrap();
        page.glyphs.push(Glyph { code: 0x41, bitmap });
        gs.pages.push(page);

        let mut doc = FontSpace::new(&mut ids, "doc", "");
        doc.character_sets.push(cs);
        doc.glyph_sets.push(gs);
        doc
    }

    #[test]
    fn valid_document_has_no_errors_or_warnings() {
        let report = valid_doc().validate(&Limits::default());
        assert!(report.is_valid(), "errors: {:?}", report.errors);
        assert!(
            report.warnings.is_empty(),
            "warnings: {:?}",
            report.warnings
        );
    }

    #[test]
    fn dangling_glyph_is_a_warning_not_an_error() {
        let mut doc = valid_doc();
        // Add a glyph for a code with no character-set entry.
        let size = doc.glyph_sets[0].glyph_size;
        doc.glyph_sets[0].pages[0].glyphs.push(Glyph {
            code: 0x99,
            bitmap: Bitmap::new_blank(size),
        });
        let report = doc.validate(&Limits::default());
        assert!(report.is_valid(), "dangling must not be a hard error");
        assert_eq!(report.warnings.len(), 1);
        assert!(matches!(
            report.warnings[0],
            ValidationWarning::DanglingGlyph { code: 0x99, .. }
        ));
    }

    #[test]
    fn duplicate_glyph_code_on_a_page_is_an_error() {
        let mut doc = valid_doc();
        let size = doc.glyph_sets[0].glyph_size;
        doc.glyph_sets[0].pages[0].glyphs.push(Glyph {
            code: 0x41,
            bitmap: Bitmap::new_blank(size),
        });
        let report = doc.validate(&Limits::default());
        assert!(!report.is_valid());
        assert!(
            report
                .errors
                .iter()
                .any(|e| matches!(e, ValidationError::DuplicateGlyphCode { code: 0x41, .. }))
        );
    }

    #[test]
    fn geometry_mismatch_is_an_error() {
        let mut doc = valid_doc();
        doc.glyph_sets[0].pages[0].glyphs[0].bitmap = Bitmap::new_blank(GlyphSize::new(8, 8));
        let report = doc.validate(&Limits::default());
        assert!(
            report
                .errors
                .iter()
                .any(|e| matches!(e, ValidationError::GlyphSizeMismatch { .. }))
        );
    }

    #[test]
    fn unknown_character_set_reference_is_an_error() {
        let mut doc = valid_doc();
        // Point the glyph set at a character set that isn't in the document. Use an
        // id well clear of valid_doc's sequential ids (…0001–…0004) so it can't
        // accidentally resolve.
        doc.glyph_sets[0].character_set_id =
            crate::CharacterSetId(uuid::Uuid::from_u128(0xDEAD_BEEF));
        let report = doc.validate(&Limits::default());
        assert!(
            report
                .errors
                .iter()
                .any(|e| matches!(e, ValidationError::UnknownCharacterSet { .. }))
        );
    }

    #[test]
    fn duplicate_entry_code_is_an_error() {
        let mut doc = valid_doc();
        doc.character_sets[0].entries.push(CharacterEntry {
            code: 0x41,
            label: "A again".into(),
        });
        let report = doc.validate(&Limits::default());
        assert!(
            report
                .errors
                .iter()
                .any(|e| matches!(e, ValidationError::DuplicateEntryCode { code: 0x41, .. }))
        );
    }

    #[test]
    fn duplicate_stable_ids_are_errors() {
        let mut doc = valid_doc();
        // Clone the page (same PageId) into a second page.
        let dup_page = doc.glyph_sets[0].pages[0].clone();
        doc.glyph_sets[0].pages.push(dup_page);
        let report = doc.validate(&Limits::default());
        assert!(
            report
                .errors
                .iter()
                .any(|e| matches!(e, ValidationError::DuplicatePageId(_)))
        );
    }

    #[test]
    fn duplicate_guide_ids_are_errors() {
        let mut doc = valid_doc();
        let mut ids = SequentialIdGen::new();
        let guide = Guide {
            id: crate::GuideId::new(&mut ids),
            name: "baseline".into(),
            axis: GuideAxis::Horizontal,
            position: 12,
            visible: true,
            locked: false,
        };
        doc.glyph_sets[0].pages[0].guides.push(guide.clone());
        doc.glyph_sets[0].pages[0].guides.push(guide);
        let report = doc.validate(&Limits::default());
        assert!(
            report
                .errors
                .iter()
                .any(|e| matches!(e, ValidationError::DuplicateGuideId(_)))
        );
    }

    #[test]
    fn invalid_glyph_size_is_an_error() {
        let mut doc = valid_doc();
        doc.glyph_sets[0].glyph_size = GlyphSize::new(0, 16);
        let report = doc.validate(&Limits::default());
        assert!(
            report
                .errors
                .iter()
                .any(|e| matches!(e, ValidationError::GlyphSizeInvalid { .. }))
        );
    }

    #[test]
    fn wrong_format_version_is_an_error() {
        let mut doc = valid_doc();
        doc.format_version = 999;
        let report = doc.validate(&Limits::default());
        assert!(report.errors.iter().any(|e| matches!(
            e,
            ValidationError::UnsupportedFormatVersion { found: 999, .. }
        )));
    }

    #[test]
    fn resource_limits_are_enforced_and_cited() {
        let doc = valid_doc();
        // Tiny limits: 1 entry/set, 0 pages/set, 1 total pixel.
        let limits = Limits {
            max_character_slots_per_set: 1,
            max_pages_per_glyph_set: 0,
            max_total_glyph_pixels_per_document: 1,
            ..Limits::default()
        };
        let report = doc.validate(&limits);
        assert!(
            report
                .errors
                .iter()
                .any(|e| matches!(e, ValidationError::TooManyEntries { max: 1, .. }))
        );
        assert!(
            report
                .errors
                .iter()
                .any(|e| matches!(e, ValidationError::TooManyPages { max: 0, .. }))
        );
        assert!(
            report
                .errors
                .iter()
                .any(|e| matches!(e, ValidationError::TotalGlyphPixelsExceeded { max: 1, .. }))
        );
    }

    #[test]
    fn error_messages_carry_object_context() {
        // spec/14 §14.4: the human string is rendered from structured context.
        let err = ValidationError::GlyphSizeMismatch {
            glyph_set: {
                let mut ids = SequentialIdGen::new();
                crate::GlyphSetId::new(&mut ids)
            },
            page: {
                let mut ids = SequentialIdGen::new();
                crate::PageId::new(&mut ids)
            },
            code: 0x41,
            expected: GlyphSize::new(8, 16),
            found: GlyphSize::new(8, 8),
        };
        let rendered = err.to_string();
        assert!(rendered.contains("0x0041"));
        assert!(rendered.contains("8×16"));
        assert!(rendered.contains("8×8"));
    }
}
