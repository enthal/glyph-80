//! Structured load/save errors (spec/06, spec/14 §14.4).
//!
//! Every variant names the object context as specifically as the failing site
//! allows — glyph set, page, code, row, column — so a front end can render a
//! precise message. Save is infallible (a domain document is always serializable);
//! only [`load`](crate::load) can fail.

use fontspace_model::ValidationError;

/// Why loading a `.fontspace.json` document failed.
#[derive(Debug, thiserror::Error)]
pub enum JsonError {
    /// The bytes are not valid JSON, or do not match the storage schema.
    #[error("JSON syntax/schema error: {0}")]
    Syntax(#[from] serde_json::Error),

    /// The document declares a `format_version` this build does not support.
    #[error("unsupported document format_version {found}; this build supports {supported}")]
    UnsupportedVersion { found: u32, supported: u32 },

    /// A UUID string could not be parsed.
    #[error("{context}: invalid UUID {value:?}")]
    InvalidUuid { context: String, value: String },

    /// A `code` string was neither decimal nor `0x` hex.
    #[error("{context}: invalid code {value:?} (expected decimal or 0x hex)")]
    InvalidCode { context: String, value: String },

    /// A glyph's `pixels` row count did not equal the glyph set's height.
    #[error("{context}: expected {expected} pixel rows, found {found}")]
    RowCount {
        context: String,
        expected: u16,
        found: usize,
    },

    /// A glyph's `pixels` row width did not equal the glyph set's width.
    #[error("{context}: row {row}: expected {expected} pixels, found {found}")]
    RowWidth {
        context: String,
        row: usize,
        expected: u16,
        found: usize,
    },

    /// A `pixels` row contained a character other than `.` or `#`.
    #[error("{context}: row {row}, column {column}: invalid pixel {ch:?} (expected '.' or '#')")]
    InvalidPixelChar {
        context: String,
        row: usize,
        column: usize,
        ch: char,
    },

    /// The document parsed but failed structural validation (spec/14 §14.1).
    /// Dangling glyphs do **not** appear here — they are tolerated warnings.
    #[error("document failed validation with {} error(s)", .0.len())]
    Invalid(Vec<ValidationError>),

    /// A fragment declares a `fragment_version` this build does not support (spec/08 §8.5).
    #[error("unsupported fragment_version {found}; this build supports {supported}")]
    UnsupportedFragmentVersion { found: u32, supported: u32 },

    /// A fragment declares a `kind` this build does not recognize (spec/08 §8.1/§8.5).
    #[error("unknown fragment kind {kind:?}")]
    UnknownFragmentKind { kind: String },

    /// A fragment's `source_glyph_size` is not a valid geometry (spec/03 §3.4).
    #[error("fragment source_glyph_size {width}×{height} is invalid: {reason}")]
    InvalidFragmentGeometry {
        width: u16,
        height: u16,
        reason: String,
    },
}
