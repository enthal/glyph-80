#![forbid(unsafe_code)]

//! Core document and value types for FontSpace.
//!
//! This crate is the base of the inward-dependency stack (spec/02): it defines the
//! value types, typed IDs, id injection, and the `Bitmap`, and depends on no other
//! FontSpace crate and on nothing GUI. The document aggregate (character sets,
//! glyph sets, pages, glyphs, guides) and operations land in later Milestone-1
//! slices; this slice establishes the foundations they build on.
//!
//! See `FontSpace/spec/` — start with `17-invariants-and-glossary.md`, then `03`,
//! `05`, and `16`.

mod bitmap;
mod character_set;
mod color;
mod document;
mod export_config;
mod fragment;
mod geometry;
mod glyph_set;
mod ids;
mod limits;
mod validation;

pub use bitmap::{Bitmap, BitmapError, OverflowPolicy, flipped, inverted, shifted};
pub use character_set::{CharacterEntry, CharacterSet};
pub use color::Rgba;
pub use document::{CURRENT_FORMAT_VERSION, FontSpace, FontSpaceMetadata};
pub use export_config::ExportConfig;
pub use fragment::{FontSpaceFragment, FragmentGlyph, GlyphFragment};
pub use geometry::{GlyphSize, GlyphSizeError, GuideAxis};
pub use glyph_set::{Glyph, GlyphPage, GlyphSet, Guide};
pub use ids::{
    CharacterSetId, ExportComponentId, ExportConfigId, FontSpaceId, GlyphSetId, GuideId, IdGen,
    PageId, RandomIdGen, SequentialIdGen,
};
pub use limits::Limits;
pub use validation::{ValidationError, ValidationReport, ValidationWarning};
