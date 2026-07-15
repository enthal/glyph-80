//! Rectangular pixel-region selection and its in-place transforms (spec/12 §12.4).
//!
//! Pure logic — a marquee rectangle over glyph cells, and the [`SetPixels`] edits a
//! region transform produces — tested outside the editor's paint/input code, exactly
//! as [`stroke`](super::stroke) computes the cells a drag touches. The editor turns
//! these edits into a `SetPixels` domain op, so the document is only ever mutated
//! through `fontspace-ops` (the one architectural rule).

use fontspace_model::Bitmap;
use fontspace_ops::PixelEdit;

/// A rectangular selection of glyph cells, **inclusive** of both corners.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PixelRect {
    pub x0: u16,
    pub y0: u16,
    pub x1: u16,
    pub y1: u16,
}

impl PixelRect {
    /// The normalized rectangle spanning two corner cells (either drag order).
    pub fn from_corners(a: (u16, u16), b: (u16, u16)) -> Self {
        Self {
            x0: a.0.min(b.0),
            y0: a.1.min(b.1),
            x1: a.0.max(b.0),
            y1: a.1.max(b.1),
        }
    }

    pub fn width(&self) -> u16 {
        self.x1 - self.x0 + 1
    }

    pub fn height(&self) -> u16 {
        self.y1 - self.y0 + 1
    }
}

/// Which way to mirror a region — the "reverse" the user asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlipDir {
    /// Mirror left↔right about the selection's vertical center.
    LeftRight,
    /// Mirror top↔bottom about the selection's horizontal center.
    TopBottom,
}

/// The `SetPixels` edits that mirror `rect` of `bitmap` in place. Each cell takes the
/// value of its mirror partner **within the rect**; cells outside the rect are
/// untouched. Values are read from `bitmap` (a snapshot), so applying the whole edit
/// list at once is a correct flip. The mirror partner is always inside the rect (and
/// therefore in bounds), so no coordinate escapes the glyph.
pub fn flip_edits(bitmap: &Bitmap, rect: PixelRect, dir: FlipDir) -> Vec<PixelEdit> {
    let mut edits = Vec::with_capacity(rect.width() as usize * rect.height() as usize);
    for y in rect.y0..=rect.y1 {
        for x in rect.x0..=rect.x1 {
            let (sx, sy) = match dir {
                FlipDir::LeftRight => (rect.x0 + rect.x1 - x, y),
                FlipDir::TopBottom => (x, rect.y0 + rect.y1 - y),
            };
            edits.push(PixelEdit {
                x,
                y,
                value: bitmap.get(sx, sy).unwrap_or(false),
            });
        }
    }
    edits
}

#[cfg(test)]
mod tests {
    use super::*;
    use fontspace_model::GlyphSize;

    fn bitmap_with(size: u16, on: &[(u16, u16)]) -> Bitmap {
        let mut b = Bitmap::new_blank(GlyphSize::new(size, size));
        for &(x, y) in on {
            b.set(x, y, true).unwrap();
        }
        b
    }

    /// Applies the edits to a clone and returns it, mimicking `set_pixels`.
    fn apply(bitmap: &Bitmap, edits: &[PixelEdit]) -> Bitmap {
        let mut out = bitmap.clone();
        for e in edits {
            out.set(e.x, e.y, e.value).unwrap();
        }
        out
    }

    #[test]
    fn from_corners_normalizes_either_drag_order() {
        let a = PixelRect::from_corners((5, 6), (2, 1));
        let b = PixelRect::from_corners((2, 1), (5, 6));
        assert_eq!(a, b);
        assert_eq!((a.x0, a.y0, a.x1, a.y1), (2, 1, 5, 6));
        assert_eq!((a.width(), a.height()), (4, 6));
    }

    #[test]
    fn flip_left_right_mirrors_within_the_rect_only() {
        // 4×4 with a pixel at (0,0) and (0,3). Flip the left-half rect [0..1]×[0..3]
        // left↔right: column 0 and column 1 swap; the rest is untouched.
        let src = bitmap_with(4, &[(0, 0), (0, 3), (3, 1)]);
        let rect = PixelRect::from_corners((0, 0), (1, 3));
        let out = apply(&src, &flip_edits(&src, rect, FlipDir::LeftRight));
        // (0,0)->(1,0), (0,3)->(1,3); column 0 now empty in the rect.
        assert!(out.get(1, 0).unwrap());
        assert!(out.get(1, 3).unwrap());
        assert!(!out.get(0, 0).unwrap());
        // (3,1) is outside the rect — unchanged.
        assert!(out.get(3, 1).unwrap());
        assert_eq!(out.count_on(), 3);
    }

    #[test]
    fn flip_top_bottom_mirrors_rows() {
        // A pixel at (2,0) in a full-glyph rect flips to (2,3) in a 4×4.
        let src = bitmap_with(4, &[(2, 0)]);
        let rect = PixelRect::from_corners((0, 0), (3, 3));
        let out = apply(&src, &flip_edits(&src, rect, FlipDir::TopBottom));
        assert!(out.get(2, 3).unwrap());
        assert!(!out.get(2, 0).unwrap());
        assert_eq!(out.count_on(), 1);
    }

    #[test]
    fn flipping_twice_is_the_identity() {
        let src = bitmap_with(6, &[(1, 2), (4, 5), (0, 0)]);
        let rect = PixelRect::from_corners((0, 0), (5, 5));
        let once = apply(&src, &flip_edits(&src, rect, FlipDir::LeftRight));
        let twice = apply(&once, &flip_edits(&once, rect, FlipDir::LeftRight));
        assert_eq!(twice, src);
    }
}
