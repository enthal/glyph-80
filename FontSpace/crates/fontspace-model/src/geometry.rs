//! Glyph geometry value types (spec/03 §3.4, §3.8).

use crate::Limits;

/// A glyph geometry: pixel width and height. Both dimensions must be nonzero and
/// within [`Limits`]; enforced by [`GlyphSize::validate`] and document validation
/// (spec/14). Fields are public data — validity is a checked property, not a
/// construction guarantee.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GlyphSize {
    pub width: u16,
    pub height: u16,
}

impl GlyphSize {
    pub fn new(width: u16, height: u16) -> Self {
        Self { width, height }
    }

    /// Number of bytes one packed bitmap row occupies: `ceil(width / 8)`.
    pub fn bytes_per_row(&self) -> usize {
        (self.width as usize).div_ceil(8)
    }

    /// Total pixel count (`width * height`), widened to avoid overflow.
    pub fn pixel_count(&self) -> u64 {
        self.width as u64 * self.height as u64
    }

    /// Checks the nonzero and bounded invariants, naming the offending dimension.
    pub fn validate(&self, limits: &Limits) -> Result<(), GlyphSizeError> {
        if self.width == 0 {
            return Err(GlyphSizeError::ZeroWidth);
        }
        if self.height == 0 {
            return Err(GlyphSizeError::ZeroHeight);
        }
        if self.width > limits.max_glyph_width {
            return Err(GlyphSizeError::WidthTooLarge {
                width: self.width,
                max: limits.max_glyph_width,
            });
        }
        if self.height > limits.max_glyph_height {
            return Err(GlyphSizeError::HeightTooLarge {
                height: self.height,
                max: limits.max_glyph_height,
            });
        }
        Ok(())
    }
}

impl std::fmt::Display for GlyphSize {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}×{}", self.width, self.height)
    }
}

/// Why a [`GlyphSize`] is invalid. Each variant names the offending dimension and,
/// where relevant, the limit it exceeded (spec/14, CLAUDE.md error-context rule).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum GlyphSizeError {
    #[error("glyph width must be nonzero")]
    ZeroWidth,
    #[error("glyph height must be nonzero")]
    ZeroHeight,
    #[error("glyph width {width} exceeds the maximum of {max}")]
    WidthTooLarge { width: u16, max: u16 },
    #[error("glyph height {height} exceeds the maximum of {max}")]
    HeightTooLarge { height: u16, max: u16 },
}

/// The axis of a guide, and the mirror line for [`flipped`](crate::flipped)
/// (spec/03 §3.8, spec/05 §5.7).
///
/// - [`Horizontal`](GuideAxis::Horizontal) is a horizontal line of symmetry — a
///   flip across it mirrors top↔bottom (row `y` ↔ `height-1-y`).
/// - [`Vertical`](GuideAxis::Vertical) is a vertical line of symmetry — a flip
///   across it mirrors left↔right (column `x` ↔ `width-1-x`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuideAxis {
    Horizontal,
    Vertical,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_per_row_rounds_up_for_odd_widths() {
        assert_eq!(GlyphSize::new(8, 16).bytes_per_row(), 1);
        assert_eq!(GlyphSize::new(1, 1).bytes_per_row(), 1);
        assert_eq!(GlyphSize::new(9, 1).bytes_per_row(), 2);
        assert_eq!(GlyphSize::new(16, 1).bytes_per_row(), 2);
        assert_eq!(GlyphSize::new(12, 1).bytes_per_row(), 2);
    }

    #[test]
    fn validate_rejects_zero_dimensions() {
        let limits = Limits::default();
        assert_eq!(
            GlyphSize::new(0, 16).validate(&limits),
            Err(GlyphSizeError::ZeroWidth)
        );
        assert_eq!(
            GlyphSize::new(8, 0).validate(&limits),
            Err(GlyphSizeError::ZeroHeight)
        );
    }

    #[test]
    fn validate_rejects_oversize_dimensions() {
        let limits = Limits {
            max_glyph_width: 16,
            max_glyph_height: 16,
            ..Limits::default()
        };
        assert_eq!(
            GlyphSize::new(17, 16).validate(&limits),
            Err(GlyphSizeError::WidthTooLarge { width: 17, max: 16 })
        );
        assert_eq!(
            GlyphSize::new(16, 17).validate(&limits),
            Err(GlyphSizeError::HeightTooLarge {
                height: 17,
                max: 16
            })
        );
    }

    #[test]
    fn validate_accepts_in_bounds() {
        assert_eq!(GlyphSize::new(8, 16).validate(&Limits::default()), Ok(()));
    }
}
