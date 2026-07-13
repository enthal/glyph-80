//! Resource limits, consolidated in one place (spec/16 §16.4).
//!
//! Bounds live here — not scattered as literals — so they are testable, adjustable,
//! and citable by the validator (spec/14). Defaults are deliberately well below
//! `u16::MAX` to prevent pathological allocations. Only the glyph width/height
//! bounds are consumed at Milestone 1 (via [`GlyphSize::validate`](crate::GlyphSize::validate));
//! the remaining fields are defined now for the validator, export, and fragment
//! code that lands in later milestones.

/// A set of enforced upper bounds with documented defaults.
///
/// Construct with [`Limits::default`] for the standard bounds, or build a custom
/// value in tests to exercise boundary conditions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Maximum glyph width in pixels.
    pub max_glyph_width: u16,
    /// Maximum glyph height in pixels.
    pub max_glyph_height: u16,
    /// Maximum number of character slots (entries) in one character set.
    pub max_character_slots_per_set: u32,
    /// Maximum number of pages in one glyph set.
    pub max_pages_per_glyph_set: u32,
    /// Maximum total on-or-off glyph pixels across a whole document.
    pub max_total_glyph_pixels_per_document: u64,
    /// Maximum export address width, in address bits (spec/10).
    pub max_export_address_width: u32,
    /// Maximum number of pixels in a rendered output image (spec/09).
    pub max_output_image_pixels: u64,
    /// Maximum total pixels carried by a copy/paste fragment (spec/08).
    pub max_fragment_pixels: u64,
}

impl Default for Limits {
    fn default() -> Self {
        // Rationale: raster glyphs are small (8×16, 16×16); these caps are generous
        // for real fonts while ruling out multi-gigapixel allocations. Adjust here.
        Self {
            max_glyph_width: 256,
            max_glyph_height: 256,
            max_character_slots_per_set: 65_536,
            max_pages_per_glyph_set: 256,
            max_total_glyph_pixels_per_document: 64 * 1024 * 1024,
            max_export_address_width: 24,
            max_output_image_pixels: 64 * 1024 * 1024,
            max_fragment_pixels: 16 * 1024 * 1024,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_well_below_u16_max() {
        let limits = Limits::default();
        assert!(limits.max_glyph_width < u16::MAX);
        assert!(limits.max_glyph_height < u16::MAX);
        assert!(limits.max_glyph_width > 0);
        assert!(limits.max_glyph_height > 0);
    }
}
