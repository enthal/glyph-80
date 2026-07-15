//! Serializable domain fragments for copy/paste and cross-file composition
//! (spec/08). A fragment is a detached slice of domain data — never GUI widget
//! state — carrying enough dependency metadata to be resolved into a destination
//! document by an explicit paste operation (spec/08 §8.1).
//!
//! Only the [`Glyphs`](FontSpaceFragment::Glyphs) variant exists so far; the
//! `Pages`, `Objects`, and `ExportComponents` variants (spec/08 §8.1) join it with
//! their own slices, exactly as `ObjectChange` (in `fontspace-ops`) grew
//! variant-by-variant. Fragment JSON persistence and clipboard wiring are likewise
//! later slices; this one establishes the domain types and the glyph extract/paste
//! operations that build on them.

use crate::{Bitmap, GlyphSize};

/// One glyph carried in a [`GlyphFragment`], detached from any page. Records the
/// entry `code` it rendered and that entry's `label`, so a paste can map it by code
/// or by slot into a destination whose character set differs (spec/08 §8.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FragmentGlyph {
    pub code: u32,
    pub label: String,
    pub bitmap: Bitmap,
}

/// A slice of glyphs copied from one geometry (spec/08 §8.1). The
/// `source_glyph_size` lets a paste enforce geometry agreement (the default
/// `RequireExact`) or, once size conversions land, apply an explicit one — no
/// silent resize, ever (spec/08 §8.3, invariant in spec/17).
///
/// **Invariant:** every `glyphs[i].bitmap.size()` equals `source_glyph_size` — a
/// fragment carries one geometry. `extract_glyphs` (in `fontspace-ops`) upholds
/// this by construction; a paste relies on it and the `RequireExact` check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlyphFragment {
    pub source_glyph_size: GlyphSize,
    pub glyphs: Vec<FragmentGlyph>,
}

/// A serializable slice of domain objects used for copy/paste (spec/08 §8.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FontSpaceFragment {
    Glyphs(GlyphFragment),
    // `Pages`, `Objects(Vec<FontSpaceObject>)`, and `ExportComponents` arrive with
    // their own slices (spec/08 §8.1).
}
