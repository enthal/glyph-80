//! Selectors: reusable, resolved-and-validated targets computed before any mutation
//! begins (spec/07 §7.3). Glyphs are selected primarily by `code` (unambiguous);
//! `Ordinal` selectors resolve through the referenced character set's order.

use fontspace_model::{CharacterSet, GlyphSet, PageId};

use crate::FontSpaceError;

/// Selects pages within a glyph set. `Name`/`Names` are rejected when ambiguous
/// rather than guessed (spec/07 §7.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PageSelector {
    All,
    Id(PageId),
    Ids(Vec<PageId>),
    Index(usize),
    RangeInclusive { start: usize, end: usize },
    Name(String),
    Names(Vec<String>),
}

/// Selects glyph codes. Code selectors are validated against the character set;
/// ordinal selectors resolve through entry order (spec/07 §7.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GlyphSelector {
    All,
    Code(u32),
    Codes(Vec<u32>),
    CodeRangeInclusive { start: u32, end: u32 },
    Ordinal(usize),
    Ordinals(Vec<usize>),
    OrdinalRangeInclusive { start: usize, end: usize },
}

/// Resolves a [`PageSelector`] to an ordered, validated list of page ids.
pub(crate) fn resolve_pages(
    glyph_set: &GlyphSet,
    selector: &PageSelector,
) -> Result<Vec<PageId>, FontSpaceError> {
    match selector {
        PageSelector::All => Ok(glyph_set.pages.iter().map(|page| page.id).collect()),
        PageSelector::Id(id) => {
            resolve_page_id(glyph_set, *id)?;
            Ok(vec![*id])
        }
        PageSelector::Ids(ids) => {
            for &id in ids {
                resolve_page_id(glyph_set, id)?;
            }
            Ok(ids.clone())
        }
        PageSelector::Index(index) => Ok(vec![page_at_index(glyph_set, *index)?]),
        PageSelector::RangeInclusive { start, end } => {
            if start > end {
                return Err(FontSpaceError::InvalidRange {
                    start: *start as u64,
                    end: *end as u64,
                });
            }
            (*start..=*end)
                .map(|index| page_at_index(glyph_set, index))
                .collect()
        }
        PageSelector::Name(name) => Ok(vec![resolve_page_name(glyph_set, name)?]),
        PageSelector::Names(names) => names
            .iter()
            .map(|name| resolve_page_name(glyph_set, name))
            .collect(),
    }
}

fn resolve_page_id(glyph_set: &GlyphSet, id: PageId) -> Result<(), FontSpaceError> {
    if glyph_set.page_of_id(id).is_some() {
        Ok(())
    } else {
        Err(FontSpaceError::PageIdNotFound {
            glyph_set: glyph_set.id,
            page: id,
        })
    }
}

fn page_at_index(glyph_set: &GlyphSet, index: usize) -> Result<PageId, FontSpaceError> {
    glyph_set
        .pages
        .get(index)
        .map(|page| page.id)
        .ok_or(FontSpaceError::PageIndexOutOfRange {
            glyph_set: glyph_set.id,
            index,
            len: glyph_set.pages.len(),
        })
}

fn resolve_page_name(glyph_set: &GlyphSet, name: &str) -> Result<PageId, FontSpaceError> {
    let mut matches = glyph_set.pages.iter().filter(|page| page.name == name);
    let first = matches
        .next()
        .ok_or_else(|| FontSpaceError::PageNameNotFound {
            glyph_set: glyph_set.id,
            name: name.to_string(),
        })?;
    let count = 1 + matches.count();
    if count > 1 {
        return Err(FontSpaceError::AmbiguousPageName {
            glyph_set: glyph_set.id,
            name: name.to_string(),
            count,
        });
    }
    Ok(first.id)
}

/// Resolves a [`GlyphSelector`] to an ordered, validated list of codes. Code-based
/// selectors reject codes with no entry; range/ordinal selectors derive from entry
/// order and so are always valid (spec/07 §7.3).
pub(crate) fn resolve_glyph_codes(
    character_set: &CharacterSet,
    selector: &GlyphSelector,
) -> Result<Vec<u32>, FontSpaceError> {
    match selector {
        GlyphSelector::All => Ok(character_set
            .entries
            .iter()
            .map(|entry| entry.code)
            .collect()),
        GlyphSelector::Code(code) => {
            require_code(character_set, *code)?;
            Ok(vec![*code])
        }
        GlyphSelector::Codes(codes) => {
            for &code in codes {
                require_code(character_set, code)?;
            }
            Ok(codes.clone())
        }
        GlyphSelector::CodeRangeInclusive { start, end } => {
            if start > end {
                return Err(FontSpaceError::InvalidRange {
                    start: *start as u64,
                    end: *end as u64,
                });
            }
            // Entry codes falling within the numeric range, in entry order.
            Ok(character_set
                .entries
                .iter()
                .map(|entry| entry.code)
                .filter(|code| (*start..=*end).contains(code))
                .collect())
        }
        GlyphSelector::Ordinal(ordinal) => Ok(vec![code_at_ordinal(character_set, *ordinal)?]),
        GlyphSelector::Ordinals(ordinals) => ordinals
            .iter()
            .map(|&o| code_at_ordinal(character_set, o))
            .collect(),
        GlyphSelector::OrdinalRangeInclusive { start, end } => {
            if start > end {
                return Err(FontSpaceError::InvalidRange {
                    start: *start as u64,
                    end: *end as u64,
                });
            }
            (*start..=*end)
                .map(|o| code_at_ordinal(character_set, o))
                .collect()
        }
    }
}

fn require_code(character_set: &CharacterSet, code: u32) -> Result<(), FontSpaceError> {
    if character_set.contains_code(code) {
        Ok(())
    } else {
        Err(FontSpaceError::CodeNotInCharacterSet {
            character_set: character_set.id,
            code,
        })
    }
}

fn code_at_ordinal(character_set: &CharacterSet, ordinal: usize) -> Result<u32, FontSpaceError> {
    character_set
        .entries
        .get(ordinal)
        .map(|entry| entry.code)
        .ok_or(FontSpaceError::OrdinalOutOfRange {
            character_set: character_set.id,
            ordinal,
            len: character_set.entries.len(),
        })
}
