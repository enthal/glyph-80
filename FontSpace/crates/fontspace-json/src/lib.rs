#![forbid(unsafe_code)]

//! Deterministic, canonical JSON persistence for FontSpace documents (spec/06).
//!
//! [`save`] converts a domain [`FontSpace`] to the storage schema and writes
//! canonical, diff-friendly JSON: stable field order (from explicit `Stored*`
//! structs), glyphs in charset-entry order with blanks pruned, `code`s as `0x` hex,
//! bitmaps as `.`/`#` visual rows, 2-space indent, lowercase UUIDs, and exactly one
//! trailing newline. The determinism contract is `save(load(save(d))) == save(d)`,
//! byte-for-byte.
//!
//! [`load`] parses (dispatching on `format_version`), converts back to domain types
//! with precise per-object errors, and runs document validation. A glyph whose
//! `code` has no entry is tolerated and reported as a warning (spec/04 §4.4); hard
//! structural problems fail the load.

mod error;
pub mod file;
mod fragment;
mod pixels;
mod storage;

use std::collections::HashMap;

use fontspace_model::{
    CURRENT_FORMAT_VERSION, CharacterEntry, CharacterSet, CharacterSetId, ExportConfig,
    ExportConfigId, FontSpace, FontSpaceId, FontSpaceMetadata, Glyph, GlyphPage, GlyphSet,
    GlyphSetId, GlyphSize, Guide, GuideAxis, GuideId, Limits, PageId, ValidationWarning,
};
use uuid::Uuid;

pub use error::JsonError;
pub use file::{ReadError, read_document, read_fragment, write_document, write_fragment};
pub use fragment::{CURRENT_FRAGMENT_VERSION, load_fragment, save_fragment};

use pixels::{RowError, format_code, format_rows, parse_code, parse_rows};
use storage::*;

/// The result of a successful [`load`]: the document plus any tolerated warnings
/// (e.g. dangling glyphs) surfaced during validation.
#[derive(Debug, Clone)]
pub struct LoadOutcome {
    pub document: FontSpace,
    pub warnings: Vec<ValidationWarning>,
}

/// Serialize a document to canonical JSON (spec/06). Infallible: a domain document
/// is always representable, and the `Stored*` schema serializes without error.
pub fn save(doc: &FontSpace) -> String {
    let stored = to_stored(doc);
    let mut json = serde_json::to_string_pretty(&stored).expect(
        "Stored* schema (plain structs of strings, numbers, bools, vecs) serializes infallibly",
    );
    json.push('\n');
    json
}

/// Parse canonical JSON into a document, validating structure. Dangling glyphs are
/// tolerated (returned as warnings); hard structural errors fail the load (spec/06,
/// spec/14 §14.1).
pub fn load(text: &str) -> Result<LoadOutcome, JsonError> {
    let peek: VersionPeek = serde_json::from_str(text)?;
    match peek.format_version {
        1 => {
            let stored: StoredFontSpaceV1 = serde_json::from_str(text)?;
            from_stored_v1(stored)
        }
        other => Err(JsonError::UnsupportedVersion {
            found: other,
            supported: CURRENT_FORMAT_VERSION,
        }),
    }
}

// --- Domain -> storage (save) ---

fn to_stored(doc: &FontSpace) -> StoredFontSpaceV1 {
    StoredFontSpaceV1 {
        format_version: doc.format_version,
        id: uuid_string(doc.id.as_uuid()),
        metadata: StoredMetadata {
            name: doc.metadata.name.clone(),
            description: doc.metadata.description.clone(),
        },
        character_sets: doc
            .character_sets
            .iter()
            .map(to_stored_character_set)
            .collect(),
        glyph_sets: doc
            .glyph_sets
            .iter()
            .map(|glyph_set| to_stored_glyph_set(glyph_set, doc))
            .collect(),
        export_configs: doc
            .export_configs
            .iter()
            .map(to_stored_export_config)
            .collect(),
    }
}

fn to_stored_character_set(character_set: &CharacterSet) -> StoredCharacterSet {
    StoredCharacterSet {
        id: uuid_string(character_set.id.as_uuid()),
        name: character_set.name.clone(),
        description: character_set.description.clone(),
        entries: character_set
            .entries
            .iter()
            .map(|entry| StoredCharacterEntry {
                code: format_code(entry.code),
                label: entry.label.clone(),
            })
            .collect(),
    }
}

fn to_stored_glyph_set(glyph_set: &GlyphSet, doc: &FontSpace) -> StoredGlyphSet {
    // Canonical glyph order is the referenced character set's entry order.
    let entry_order: Vec<u32> = doc
        .character_set(glyph_set.character_set_id)
        .map(|cs| cs.entries.iter().map(|e| e.code).collect())
        .unwrap_or_default();
    StoredGlyphSet {
        id: uuid_string(glyph_set.id.as_uuid()),
        name: glyph_set.name.clone(),
        description: glyph_set.description.clone(),
        glyph_size: StoredGlyphSize {
            width: glyph_set.glyph_size.width,
            height: glyph_set.glyph_size.height,
        },
        character_set_id: uuid_string(glyph_set.character_set_id.as_uuid()),
        pages: glyph_set
            .pages
            .iter()
            .map(|page| to_stored_page(page, &entry_order))
            .collect(),
    }
}

fn to_stored_page(page: &GlyphPage, entry_order: &[u32]) -> StoredGlyphPage {
    StoredGlyphPage {
        id: uuid_string(page.id.as_uuid()),
        name: page.name.clone(),
        description: page.description.clone(),
        guides: page.guides.iter().map(to_stored_guide).collect(),
        glyphs: ordered_stored_glyphs(page, entry_order),
    }
}

/// Glyphs in charset-entry order, blanks pruned; any dangling glyphs (codes with no
/// entry) follow in ascending code order so output is fully deterministic (spec/06).
fn ordered_stored_glyphs(page: &GlyphPage, entry_order: &[u32]) -> Vec<StoredGlyph> {
    let ordinal_by_code: HashMap<u32, usize> = entry_order
        .iter()
        .enumerate()
        .map(|(i, &code)| (code, i))
        .collect();
    let mut in_order: Vec<(usize, &Glyph)> = Vec::new();
    let mut dangling: Vec<&Glyph> = Vec::new();
    for glyph in &page.glyphs {
        if glyph.bitmap.is_blank() {
            continue; // prune-on-save (spec/05 §5.6)
        }
        match ordinal_by_code.get(&glyph.code) {
            Some(&ordinal) => in_order.push((ordinal, glyph)),
            None => dangling.push(glyph),
        }
    }
    in_order.sort_by_key(|(ordinal, _)| *ordinal);
    dangling.sort_by_key(|glyph| glyph.code);
    in_order
        .into_iter()
        .map(|(_, glyph)| to_stored_glyph(glyph))
        .chain(dangling.into_iter().map(to_stored_glyph))
        .collect()
}

fn to_stored_glyph(glyph: &Glyph) -> StoredGlyph {
    StoredGlyph {
        code: format_code(glyph.code),
        pixels: format_rows(&glyph.bitmap),
    }
}

fn to_stored_guide(guide: &Guide) -> StoredGuide {
    StoredGuide {
        id: uuid_string(guide.id.as_uuid()),
        name: guide.name.clone(),
        axis: match guide.axis {
            GuideAxis::Horizontal => StoredAxis::Horizontal,
            GuideAxis::Vertical => StoredAxis::Vertical,
        },
        position: guide.position,
        visible: guide.visible,
        locked: guide.locked,
    }
}

fn to_stored_export_config(export_config: &ExportConfig) -> StoredExportConfig {
    StoredExportConfig {
        id: uuid_string(export_config.id.as_uuid()),
        name: export_config.name.clone(),
        description: export_config.description.clone(),
    }
}

fn uuid_string(uuid: Uuid) -> String {
    // Uuid::to_string is lowercase, hyphenated — the canonical form (spec/06 §6.1).
    uuid.to_string()
}

// --- Storage -> domain (load) ---

fn from_stored_v1(stored: StoredFontSpaceV1) -> Result<LoadOutcome, JsonError> {
    let StoredFontSpaceV1 {
        format_version,
        id,
        metadata,
        character_sets,
        glyph_sets,
        export_configs,
    } = stored;

    let document = FontSpace {
        format_version,
        id: FontSpaceId(parse_uuid("document id", &id)?),
        metadata: FontSpaceMetadata {
            name: metadata.name,
            description: metadata.description,
        },
        character_sets: character_sets
            .into_iter()
            .map(from_stored_character_set)
            .collect::<Result<_, _>>()?,
        glyph_sets: glyph_sets
            .into_iter()
            .map(from_stored_glyph_set)
            .collect::<Result<_, _>>()?,
        export_configs: export_configs
            .into_iter()
            .map(from_stored_export_config)
            .collect::<Result<_, _>>()?,
    };

    let report = document.validate(&Limits::default());
    if !report.errors.is_empty() {
        return Err(JsonError::Invalid(report.errors));
    }
    Ok(LoadOutcome {
        document,
        warnings: report.warnings,
    })
}

fn from_stored_character_set(stored: StoredCharacterSet) -> Result<CharacterSet, JsonError> {
    let StoredCharacterSet {
        id,
        name,
        description,
        entries,
    } = stored;
    let context = format!("character set {name:?}");
    let id = CharacterSetId(parse_uuid(&context, &id)?);
    let entries = entries
        .into_iter()
        .map(|entry| {
            let entry_context = format!("{context} entry {:?}", entry.label);
            Ok(CharacterEntry {
                code: parse_code_in(&entry_context, &entry.code)?,
                label: entry.label,
            })
        })
        .collect::<Result<Vec<_>, JsonError>>()?;
    Ok(CharacterSet {
        id,
        name,
        description,
        entries,
    })
}

fn from_stored_glyph_set(stored: StoredGlyphSet) -> Result<GlyphSet, JsonError> {
    let StoredGlyphSet {
        id,
        name,
        description,
        glyph_size,
        character_set_id,
        pages,
    } = stored;
    let context = format!("glyph set {name:?}");
    let id = GlyphSetId(parse_uuid(&context, &id)?);
    let character_set_id = CharacterSetId(parse_uuid(
        &format!("{context} character_set_id"),
        &character_set_id,
    )?);
    let glyph_size = GlyphSize::new(glyph_size.width, glyph_size.height);
    let pages = pages
        .into_iter()
        .map(|page| from_stored_page(page, &name, glyph_size))
        .collect::<Result<Vec<_>, JsonError>>()?;
    Ok(GlyphSet {
        id,
        name,
        description,
        glyph_size,
        character_set_id,
        pages,
    })
}

fn from_stored_page(
    stored: StoredGlyphPage,
    glyph_set_name: &str,
    glyph_size: GlyphSize,
) -> Result<GlyphPage, JsonError> {
    let StoredGlyphPage {
        id,
        name,
        description,
        guides,
        glyphs,
    } = stored;
    let context = format!("glyph set {glyph_set_name:?} / page {name:?}");
    let id = PageId(parse_uuid(&context, &id)?);
    let guides = guides
        .into_iter()
        .map(|guide| from_stored_guide(guide, &context))
        .collect::<Result<Vec<_>, JsonError>>()?;
    let glyphs = glyphs
        .into_iter()
        .map(|glyph| from_stored_glyph(glyph, &context, glyph_size))
        .collect::<Result<Vec<_>, JsonError>>()?;
    Ok(GlyphPage {
        id,
        name,
        description,
        guides,
        glyphs,
    })
}

fn from_stored_guide(stored: StoredGuide, page_context: &str) -> Result<Guide, JsonError> {
    let StoredGuide {
        id,
        name,
        axis,
        position,
        visible,
        locked,
    } = stored;
    let context = format!("{page_context} / guide {name:?}");
    Ok(Guide {
        id: GuideId(parse_uuid(&context, &id)?),
        name,
        axis: match axis {
            StoredAxis::Horizontal => GuideAxis::Horizontal,
            StoredAxis::Vertical => GuideAxis::Vertical,
        },
        position,
        visible,
        locked,
    })
}

fn from_stored_glyph(
    stored: StoredGlyph,
    page_context: &str,
    glyph_size: GlyphSize,
) -> Result<Glyph, JsonError> {
    let StoredGlyph { code, pixels } = stored;
    let code = parse_code_in(&format!("{page_context} glyph code"), &code)?;
    let context = format!("{page_context} / code {}", format_code(code));
    let bitmap = parse_rows(&pixels, glyph_size).map_err(|err| row_error_to_json(err, context))?;
    Ok(Glyph { code, bitmap })
}

fn from_stored_export_config(stored: StoredExportConfig) -> Result<ExportConfig, JsonError> {
    let StoredExportConfig {
        id,
        name,
        description,
    } = stored;
    Ok(ExportConfig {
        id: ExportConfigId(parse_uuid(&format!("export config {name:?}"), &id)?),
        name,
        description,
    })
}

fn parse_uuid(context: &str, value: &str) -> Result<Uuid, JsonError> {
    Uuid::parse_str(value).map_err(|_| JsonError::InvalidUuid {
        context: context.to_string(),
        value: value.to_string(),
    })
}

pub(crate) fn parse_code_in(context: &str, value: &str) -> Result<u32, JsonError> {
    parse_code(value).ok_or_else(|| JsonError::InvalidCode {
        context: context.to_string(),
        value: value.to_string(),
    })
}

pub(crate) fn row_error_to_json(err: RowError, context: String) -> JsonError {
    match err {
        RowError::RowCount { expected, found } => JsonError::RowCount {
            context,
            expected,
            found,
        },
        RowError::RowWidth {
            row,
            expected,
            found,
        } => JsonError::RowWidth {
            context,
            row,
            expected,
            found,
        },
        RowError::InvalidChar { row, column, ch } => JsonError::InvalidPixelChar {
            context,
            row,
            column,
            ch,
        },
    }
}

#[cfg(test)]
mod tests;
