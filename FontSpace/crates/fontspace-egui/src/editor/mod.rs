//! The glyph editor tile: a custom-painted view of the selected glyph's pixel
//! matrix, with grid lines, page guides, and a hover-coordinate readout (spec/12
//! §12.3, §12.6). It is a painted widget, **not** a grid of `Button`s.
//!
//! This slice is display-only; first-pixel-stroke editing lands in the next M2 slice
//! (spec/12 §12.4). All layout math lives in [`geometry`] and is unit-tested outside
//! this paint code.

pub mod geometry;

use egui::{Align2, Color32, CornerRadius, FontId, Rangef, Sense, Stroke, StrokeKind};

use crate::state::AppState;
use geometry::{GridLevel, MatrixGeometry};

const CELL_OFF: Color32 = Color32::from_gray(24);
const CELL_ON: Color32 = Color32::from_gray(230);
const GRID_SUBTLE: Color32 = Color32::from_gray(64);
const GRID_STRONG: Color32 = Color32::from_gray(110);
const GUIDE: Color32 = Color32::from_rgb(80, 160, 240);
const HOVER: Color32 = Color32::from_rgb(255, 200, 60);

/// Renders the glyph editor for the current selection into `ui`.
pub fn show_glyph_editor(ui: &mut egui::Ui, state: &AppState) {
    let Some((glyph_set, page)) = state.selected_context() else {
        ui.weak("No glyph selected.");
        return;
    };
    let size = glyph_set.glyph_size;
    let code = state.selection.code;

    // Header: what is being edited.
    ui.horizontal(|ui| {
        ui.strong(&glyph_set.name);
        ui.separator();
        ui.monospace(format!("code {code:#04X}"));
        if let Some(label) = state.selected_label() {
            ui.label(label);
        }
    });
    ui.separator();

    // Body: the painted matrix fills the remaining space.
    let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::hover());
    let geom = MatrixGeometry::fit(rect, size);
    let painter = ui.painter_at(rect);
    let matrix = geom.matrix_rect();

    // Cells: paint the matrix background, then only the on-pixels.
    painter.rect_filled(matrix, CornerRadius::ZERO, CELL_OFF);
    if let Some(bitmap) = state.selected_bitmap() {
        for y in 0..size.height {
            for x in 0..size.width {
                if bitmap.get(x, y).unwrap_or(false) {
                    painter.rect_filled(geom.cell_rect(x, y), CornerRadius::ZERO, CELL_ON);
                }
            }
        }
    }

    // Grid lines between/around cells (spec/12 §12.3).
    if let Some(color) = match state.grid {
        GridLevel::Off => None,
        GridLevel::Subtle => Some(GRID_SUBTLE),
        GridLevel::Strong => Some(GRID_STRONG),
    } {
        let stroke = Stroke::new(1.0, color);
        for i in 0..=size.width {
            let x = geom.origin.x + i as f32 * geom.cell_size;
            painter.vline(x, Rangef::new(matrix.top(), matrix.bottom()), stroke);
        }
        for j in 0..=size.height {
            let y = geom.origin.y + j as f32 * geom.cell_size;
            painter.hline(Rangef::new(matrix.left(), matrix.right()), y, stroke);
        }
    }

    // Page guides, drawn on the grid lines over the matrix (spec/12 §12.6).
    let guide_stroke = Stroke::new(2.0, GUIDE);
    for guide in &page.guides {
        if !guide.visible {
            continue;
        }
        let (a, b) = geom.guide_line(guide.axis, guide.position);
        painter.line_segment([a, b], guide_stroke);
    }

    // Hover: outline the cell and show its coordinate.
    if let Some((x, y)) = response.hover_pos().and_then(|pos| geom.cell_at(pos)) {
        painter.rect_stroke(
            geom.cell_rect(x, y),
            CornerRadius::ZERO,
            Stroke::new(2.0, HOVER),
            StrokeKind::Inside,
        );
        painter.text(
            matrix.left_top() + egui::vec2(4.0, 4.0),
            Align2::LEFT_TOP,
            format!("{x}, {y}"),
            FontId::monospace(12.0),
            HOVER,
        );
    }
}
