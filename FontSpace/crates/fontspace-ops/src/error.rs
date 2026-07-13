//! Structured operation errors (spec/07, spec/14 §14.2). Every variant names the
//! object context it can — glyph set, character set, page, code, coordinates.

use fontspace_model::{CharacterSetId, GlyphSetId, GuideId, PageId};

/// Why an operation could not be resolved or applied. Operations validate fully
/// before mutating, so returning one of these means the document is unchanged
/// (atomicity, spec/07 §7.6).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FontSpaceError {
    #[error("glyph set {0:?} not found")]
    GlyphSetNotFound(GlyphSetId),

    #[error(
        "glyph set {glyph_set:?} references character set {character_set:?}, which is not in the document"
    )]
    CharacterSetNotFound {
        glyph_set: GlyphSetId,
        character_set: CharacterSetId,
    },

    #[error("glyph set {glyph_set:?}: page {page:?} not found")]
    PageIdNotFound { glyph_set: GlyphSetId, page: PageId },

    #[error("glyph set {glyph_set:?}: page index {index} out of range (0..{len})")]
    PageIndexOutOfRange {
        glyph_set: GlyphSetId,
        index: usize,
        len: usize,
    },

    #[error("glyph set {glyph_set:?}: no page named {name:?}")]
    PageNameNotFound { glyph_set: GlyphSetId, name: String },

    #[error("glyph set {glyph_set:?}: page name {name:?} is ambiguous ({count} pages share it)")]
    AmbiguousPageName {
        glyph_set: GlyphSetId,
        name: String,
        count: usize,
    },

    #[error("character set {character_set:?}: code {code:#06x} has no entry")]
    CodeNotInCharacterSet {
        character_set: CharacterSetId,
        code: u32,
    },

    #[error("character set {character_set:?}: ordinal {ordinal} out of range (0..{len})")]
    OrdinalOutOfRange {
        character_set: CharacterSetId,
        ordinal: usize,
        len: usize,
    },

    #[error("invalid selector range: start {start} is greater than end {end}")]
    InvalidRange { start: u64, end: u64 },

    #[error("{context}: pixel ({x}, {y}) is out of bounds for a {width}×{height} glyph")]
    PixelOutOfBounds {
        context: String,
        x: u16,
        y: u16,
        width: u16,
        height: u16,
    },

    #[error(
        "glyph set {glyph_set:?}: reorder is not a permutation of the current {expected} page id(s)"
    )]
    InvalidPageOrder {
        glyph_set: GlyphSetId,
        expected: usize,
    },

    #[error("glyph set {glyph_set:?} / page {page:?}: guide {guide:?} not found")]
    GuideNotFound {
        glyph_set: GlyphSetId,
        page: PageId,
        guide: GuideId,
    },

    #[error("character set {0:?} not found")]
    CharacterSetIdNotFound(CharacterSetId),

    #[error("character set {character_set:?}: an entry with code {code:#06x} already exists")]
    DuplicateEntryCode {
        character_set: CharacterSetId,
        code: u32,
    },

    #[error("character set {character_set:?}: no entry with code {code:#06x}")]
    EntryCodeNotFound {
        character_set: CharacterSetId,
        code: u32,
    },

    #[error("character set {character_set:?}: entry index {index} out of range (0..={len})")]
    EntryIndexOutOfRange {
        character_set: CharacterSetId,
        index: usize,
        len: usize,
    },

    #[error(
        "character set {character_set:?}: reorder is not a permutation of the current {expected} entry code(s)"
    )]
    InvalidEntryOrder {
        character_set: CharacterSetId,
        expected: usize,
    },
}
