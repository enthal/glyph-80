//! Rectangular pixel-region selection and its in-place transforms (spec/12 §12.4).
//!
//! Pure logic — a marquee rectangle over glyph cells, and the [`SetPixels`] edits a
//! region transform produces — tested outside the editor's paint/input code, exactly
//! as [`stroke`](super::stroke) computes the cells a drag touches. The editor turns
//! these edits into a `SetPixels` domain op, so the document is only ever mutated
//! through `fontspace-ops` (the one architectural rule).

use fontspace_model::{Bitmap, GlyphSize};
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

/// Copies the pixels inside `rect` into a fresh `rect.width()`×`rect.height()` bitmap
/// — the region clipboard for copy/paste (spec/12 §12.4).
pub fn region_extract(bitmap: &Bitmap, rect: PixelRect) -> Bitmap {
    let mut out = Bitmap::new_blank(GlyphSize::new(rect.width(), rect.height()));
    for (dy, y) in (rect.y0..=rect.y1).enumerate() {
        for (dx, x) in (rect.x0..=rect.x1).enumerate() {
            if bitmap.get(x, y).unwrap_or(false) {
                // dx/dy are < rect dims, so in bounds for `out`.
                let _ = out.set(dx as u16, dy as u16, true);
            }
        }
    }
    out
}

/// The `SetPixels` edits that stamp `patch` into a `glyph`-sized bitmap with its
/// top-left at `origin`, **replacing** the destination cells (on *and* off) so the
/// whole patch rectangle is placed. Cells falling outside the glyph are clipped
/// (spec/12 §12.4). Reads only `patch`, so the edit list is a consistent stamp.
pub fn stamp_edits(patch: &Bitmap, origin: (u16, u16), glyph: GlyphSize) -> Vec<PixelEdit> {
    let mut edits = Vec::with_capacity(patch.width() as usize * patch.height() as usize);
    for py in 0..patch.height() {
        for px in 0..patch.width() {
            let x = origin.0 as u32 + px as u32;
            let y = origin.1 as u32 + py as u32;
            if x < glyph.width as u32 && y < glyph.height as u32 {
                edits.push(PixelEdit {
                    x: x as u16,
                    y: y as u16,
                    value: patch.get(px, py).unwrap_or(false),
                });
            }
        }
    }
    edits
}

/// A copy of `patch` rotated 90° **clockwise**; the result dimensions are swapped
/// (a `w`×`h` patch becomes `h`×`w`). Source `(sx, sy)` lands at `(h-1-sy, sx)`.
pub fn rotate_cw(patch: &Bitmap) -> Bitmap {
    let (w, h) = (patch.width(), patch.height());
    let mut out = Bitmap::new_blank(GlyphSize::new(h, w));
    for sy in 0..h {
        for sx in 0..w {
            if patch.get(sx, sy).unwrap_or(false) {
                let _ = out.set(h - 1 - sy, sx, true);
            }
        }
    }
    out
}

/// The `SetPixels` edits that rotate `rect` of `bitmap` 90° clockwise, anchored at the
/// rect's top-left (spec/12 §12.4): the region is cleared and the rotated patch (dims
/// swapped) is stamped at `(x0, y0)`, clipped at the glyph edge. A square selection
/// rotates in place; a non-square one rotates into its transposed footprint (so some
/// original cells outside that footprint are cleared).
pub fn rotate_edits(bitmap: &Bitmap, rect: PixelRect, glyph: GlyphSize) -> Vec<PixelEdit> {
    let rotated = rotate_cw(&region_extract(bitmap, rect));
    // Clear the whole original region first; the stamp (appended after) wins on any
    // overlapping cell, since edits apply in order.
    let mut edits: Vec<PixelEdit> = (rect.y0..=rect.y1)
        .flat_map(|y| (rect.x0..=rect.x1).map(move |x| PixelEdit { x, y, value: false }))
        .collect();
    edits.extend(stamp_edits(&rotated, (rect.x0, rect.y0), glyph));
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

    #[test]
    fn region_extract_copies_the_subrect_to_local_coords() {
        // 4×4 with (1,2) and (2,1) on; extract [1..2]×[1..2] → a 2×2 patch.
        let src = bitmap_with(4, &[(1, 2), (2, 1), (3, 3)]);
        let patch = region_extract(&src, PixelRect::from_corners((1, 1), (2, 2)));
        assert_eq!(patch.size(), GlyphSize::new(2, 2));
        assert!(patch.get(0, 1).unwrap()); // (1,2) -> local (0,1)
        assert!(patch.get(1, 0).unwrap()); // (2,1) -> local (1,0)
        assert_eq!(patch.count_on(), 2); // (3,3) is outside the rect
    }

    #[test]
    fn stamp_edits_replaces_the_destination() {
        // A 2×2 patch with only (0,0) on, stamped at (2,2) in an 8×8: it *replaces*
        // the 2×2 destination, so a previously-on cell under an off patch cell clears.
        let mut patch = Bitmap::new_blank(GlyphSize::new(2, 2));
        patch.set(0, 0, true).unwrap();
        let base = bitmap_with(8, &[(2, 2), (3, 3)]);
        let out = apply(&base, &stamp_edits(&patch, (2, 2), GlyphSize::new(8, 8)));
        assert!(out.get(2, 2).unwrap()); // patch (0,0) on
        assert!(!out.get(3, 3).unwrap()); // patch (1,1) off replaced the on pixel
        assert_eq!(out.count_on(), 1);
    }

    #[test]
    fn rotate_cw_turns_a_quarter_clockwise_and_swaps_dims() {
        // 2×3 patch (w=2, h=3) with a pixel at (0,0). CW → 3×2, pixel at (h-1, 0)=(2,0).
        let mut patch = Bitmap::new_blank(GlyphSize::new(2, 3));
        patch.set(0, 0, true).unwrap();
        let r = rotate_cw(&patch);
        assert_eq!(r.size(), GlyphSize::new(3, 2));
        assert!(r.get(2, 0).unwrap());
        assert_eq!(r.count_on(), 1);
    }

    #[test]
    fn rotate_cw_four_times_is_the_identity() {
        let src = bitmap_with(4, &[(0, 1), (2, 3), (1, 0)]);
        let r = rotate_cw(&rotate_cw(&rotate_cw(&rotate_cw(&src))));
        assert_eq!(r, src);
    }

    #[test]
    fn rotate_edits_rotates_a_square_region_in_place() {
        // A 2×2 region [0..1]×[0..1] holding a horizontal top bar (0,0),(1,0). Rotating
        // clockwise turns it into a vertical bar on the right column (1,0),(1,1).
        let src = bitmap_with(8, &[(0, 0), (1, 0)]);
        let rect = PixelRect::from_corners((0, 0), (1, 1));
        let out = apply(&src, &rotate_edits(&src, rect, GlyphSize::new(8, 8)));
        assert!(out.get(1, 0).unwrap());
        assert!(out.get(1, 1).unwrap());
        assert!(!out.get(0, 0).unwrap());
        assert_eq!(out.count_on(), 2);
    }

    #[test]
    fn stamp_edits_clips_at_the_glyph_edge() {
        // A 3×3 diagonal patch stamped at (2,2) of a 4×4 glyph: (0,0)->(2,2) and
        // (1,1)->(3,3) land; (2,2)->(4,4) is clipped away.
        let mut patch = Bitmap::new_blank(GlyphSize::new(3, 3));
        for i in 0..3 {
            patch.set(i, i, true).unwrap();
        }
        let out = apply(
            &Bitmap::new_blank(GlyphSize::new(4, 4)),
            &stamp_edits(&patch, (2, 2), GlyphSize::new(4, 4)),
        );
        assert!(out.get(2, 2).unwrap());
        assert!(out.get(3, 3).unwrap());
        assert_eq!(out.count_on(), 2);
    }
}
