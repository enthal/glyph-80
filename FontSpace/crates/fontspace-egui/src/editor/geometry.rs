//! Pure layout math for the glyph editor (spec/12 §12.3, §12.6).
//!
//! Everything here is a pure function of an available rectangle and the glyph
//! geometry — no `egui` context, no paint closure — so it is unit-tested directly
//! (CLAUDE.md: "the size math, hover mapping, and stroke logic are pure functions
//! tested outside the paint closure"). It uses `egui`'s plain `Pos2`/`Rect`/`Vec2`
//! value types, which are constructible in tests without a UI.

use egui::{Pos2, Rect, Vec2};
use fontspace_model::{GlyphSize, GuideAxis};

/// How prominently the per-pixel grid is drawn (spec/12 §12.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GridLevel {
    Off,
    Subtle,
    Strong,
}

/// The largest integer square cell size that fits `glyph` into `available`
/// (spec/12 §12.3): `floor(min(available_w / glyph_w, available_h / glyph_h))`,
/// never below 1 so a matrix always has a positive extent.
pub fn cell_size(available: Vec2, glyph: GlyphSize) -> f32 {
    let by_width = available.x / glyph.width.max(1) as f32;
    let by_height = available.y / glyph.height.max(1) as f32;
    by_width.min(by_height).floor().max(1.0)
}

/// The placed glyph matrix: square cells of `cell_size`, centered in the available
/// rectangle. All screen geometry the editor paints derives from this.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MatrixGeometry {
    pub cell_size: f32,
    /// Top-left corner of the matrix (cell (0, 0)'s min corner).
    pub origin: Pos2,
    pub glyph: GlyphSize,
}

impl MatrixGeometry {
    /// Fits and centers `glyph` within `available` (spec/12 §12.3).
    pub fn fit(available: Rect, glyph: GlyphSize) -> Self {
        let cell_size = cell_size(available.size(), glyph);
        let matrix = Vec2::new(
            cell_size * glyph.width as f32,
            cell_size * glyph.height as f32,
        );
        MatrixGeometry {
            cell_size,
            origin: available.center() - matrix / 2.0,
            glyph,
        }
    }

    /// The pixel extent of the whole matrix.
    pub fn matrix_rect(&self) -> Rect {
        Rect::from_min_size(
            self.origin,
            Vec2::new(
                self.cell_size * self.glyph.width as f32,
                self.cell_size * self.glyph.height as f32,
            ),
        )
    }

    /// The screen rectangle of cell `(x, y)` (no bounds check; callers pass valid
    /// coordinates, e.g. from a `0..width`/`0..height` loop or [`Self::cell_at`]).
    pub fn cell_rect(&self, x: u16, y: u16) -> Rect {
        Rect::from_min_size(
            self.origin + Vec2::new(x as f32 * self.cell_size, y as f32 * self.cell_size),
            Vec2::splat(self.cell_size),
        )
    }

    /// The glyph cell under `pos`, or `None` if `pos` is outside the matrix. Used for
    /// the hover readout and (later) stroke sampling.
    pub fn cell_at(&self, pos: Pos2) -> Option<(u16, u16)> {
        let local = pos - self.origin;
        if local.x < 0.0 || local.y < 0.0 {
            return None;
        }
        let x = (local.x / self.cell_size) as u32;
        let y = (local.y / self.cell_size) as u32;
        if x < self.glyph.width as u32 && y < self.glyph.height as u32 {
            Some((x as u16, y as u16))
        } else {
            None
        }
    }

    /// The endpoints of a guide line at grid-line `position` (spec/12 §12.6). A guide
    /// is drawn *between* pixels, on a grid line: a vertical guide at `position = n`
    /// is the vertical line left of column `n` (`x = origin.x + n·cell`); a
    /// horizontal guide is the horizontal line above row `n`. `position` may be
    /// negative or beyond the glyph bounds, in which case the line lies outside the
    /// matrix.
    pub fn guide_line(&self, axis: GuideAxis, position: i32) -> (Pos2, Pos2) {
        let rect = self.matrix_rect();
        let offset = position as f32 * self.cell_size;
        match axis {
            GuideAxis::Vertical => {
                let x = self.origin.x + offset;
                (Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom()))
            }
            GuideAxis::Horizontal => {
                let y = self.origin.y + offset;
                (Pos2::new(rect.left(), y), Pos2::new(rect.right(), y))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect::from_min_size(Pos2::new(x, y), Vec2::new(w, h))
    }

    #[test]
    fn cell_size_is_floored_min_dimension() {
        // 100/8 = 12.5 → 12; the smaller dimension governs.
        assert_eq!(
            cell_size(Vec2::new(100.0, 100.0), GlyphSize::new(8, 8)),
            12.0
        );
        assert_eq!(cell_size(Vec2::new(100.0, 60.0), GlyphSize::new(8, 8)), 7.0);
        // Odd width is handled by the same min/floor.
        assert_eq!(
            cell_size(Vec2::new(50.0, 100.0), GlyphSize::new(5, 8)),
            10.0
        );
    }

    #[test]
    fn cell_size_never_below_one() {
        assert_eq!(cell_size(Vec2::new(4.0, 4.0), GlyphSize::new(8, 8)), 1.0);
        assert_eq!(cell_size(Vec2::new(0.0, 0.0), GlyphSize::new(8, 8)), 1.0);
    }

    #[test]
    fn matrix_is_centered_in_available() {
        let geom = MatrixGeometry::fit(rect(0.0, 0.0, 100.0, 100.0), GlyphSize::new(8, 8));
        // cell 12 → matrix 96×96, centered in 100×100 → origin (2, 2).
        assert_eq!(geom.cell_size, 12.0);
        assert_eq!(geom.origin, Pos2::new(2.0, 2.0));
        assert_eq!(geom.matrix_rect(), rect(2.0, 2.0, 96.0, 96.0));
    }

    #[test]
    fn cell_rect_places_each_cell() {
        let geom = MatrixGeometry::fit(rect(0.0, 0.0, 100.0, 100.0), GlyphSize::new(8, 8));
        assert_eq!(geom.cell_rect(0, 0), rect(2.0, 2.0, 12.0, 12.0));
        assert_eq!(geom.cell_rect(1, 2), rect(14.0, 26.0, 12.0, 12.0));
    }

    #[test]
    fn cell_at_maps_hover_and_rejects_outside() {
        let geom = MatrixGeometry::fit(rect(0.0, 0.0, 100.0, 100.0), GlyphSize::new(8, 8));
        assert_eq!(geom.cell_at(Pos2::new(3.0, 3.0)), Some((0, 0)));
        assert_eq!(geom.cell_at(Pos2::new(15.0, 27.0)), Some((1, 2)));
        // Just left of / above the matrix, and past its right edge.
        assert_eq!(geom.cell_at(Pos2::new(1.0, 1.0)), None);
        assert_eq!(geom.cell_at(Pos2::new(99.0, 50.0)), None);
        // Bottom-right corner cell.
        assert_eq!(geom.cell_at(Pos2::new(97.0, 97.0)), Some((7, 7)));
    }

    #[test]
    fn guide_lines_sit_on_grid_boundaries() {
        let geom = MatrixGeometry::fit(rect(0.0, 0.0, 100.0, 100.0), GlyphSize::new(8, 8));
        // Vertical guide left of column 4 → x = 2 + 4·12 = 50, full height.
        assert_eq!(
            geom.guide_line(GuideAxis::Vertical, 4),
            (Pos2::new(50.0, 2.0), Pos2::new(50.0, 98.0))
        );
        // Horizontal guide above row 0 → the matrix's top edge.
        assert_eq!(
            geom.guide_line(GuideAxis::Horizontal, 0),
            (Pos2::new(2.0, 2.0), Pos2::new(98.0, 2.0))
        );
        // A negative position draws above the matrix (outside).
        let (a, _b) = geom.guide_line(GuideAxis::Horizontal, -1);
        assert_eq!(a.y, 2.0 - 12.0);
    }
}
