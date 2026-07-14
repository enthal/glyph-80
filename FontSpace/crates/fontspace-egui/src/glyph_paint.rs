//! Shared glyph painting: draw a `Bitmap` into a rectangle as square cells. Used by
//! the glyph editor's matrix, the page-overview thumbnails, and the text preview so
//! the three stay visually in lockstep (spec/12).

use egui::{Color32, CornerRadius, Painter, Rect};
use fontspace_model::{Bitmap, GlyphSize};

use crate::editor::geometry::MatrixGeometry;

/// A cell with the pixel off (the matrix background).
pub const CELL_OFF: Color32 = Color32::from_gray(24);
/// A cell with the pixel on.
pub const CELL_ON: Color32 = Color32::from_gray(230);

/// Paints `bitmap` into `rect`: fills the off background, then the on-pixels as
/// square cells sized/centered by [`MatrixGeometry::fit`]. `None`/absent glyph paints
/// a blank cell (spec/05 §5.6).
pub fn paint_bitmap(painter: &Painter, rect: Rect, bitmap: Option<&Bitmap>, size: GlyphSize) {
    painter.rect_filled(rect, CornerRadius::ZERO, CELL_OFF);
    let Some(bitmap) = bitmap else {
        return;
    };
    let geom = MatrixGeometry::fit(rect, size);
    for y in 0..size.height {
        for x in 0..size.width {
            if bitmap.get(x, y).unwrap_or(false) {
                painter.rect_filled(geom.cell_rect(x, y), CornerRadius::ZERO, CELL_ON);
            }
        }
    }
}
