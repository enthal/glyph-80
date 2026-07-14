//! The glyph editor tile: a custom-painted view of the selected glyph's pixel
//! matrix, with grid lines, page guides, a hover-coordinate readout, and
//! first-pixel-determines-stroke pixel editing (spec/12 §12.3–§12.4). It is a
//! painted widget, **not** a matrix of `Button`s.
//!
//! All layout and stroke math lives in [`geometry`] and [`stroke`] and is unit-tested
//! outside this paint/input code.

pub mod geometry;
pub mod stroke;

use egui::{
    Align2, Color32, CornerRadius, FontId, Rangef, Sense, Stroke as EguiStroke, StrokeKind,
};
use fontspace_model::{GuideAxis, GuideId};

use crate::state::AppState;
use geometry::{GridLevel, MatrixGeometry};

const CELL_OFF: Color32 = Color32::from_gray(24);
const CELL_ON: Color32 = Color32::from_gray(230);
const GRID_SUBTLE: Color32 = Color32::from_gray(64);
const GRID_STRONG: Color32 = Color32::from_gray(110);
const GUIDE: Color32 = Color32::from_rgb(80, 160, 240);
const HOVER: Color32 = Color32::from_rgb(255, 200, 60);
/// Tentative paint (a stroke in progress, before commit).
const TENTATIVE_ON: Color32 = Color32::from_rgb(180, 210, 120);
const TENTATIVE_OFF: Color32 = Color32::from_rgb(70, 60, 40);

/// Renders the glyph editor for the current selection into `ui`, handling pointer
/// editing. Mutates `state` (applies a committed stroke, tracks the in-progress one).
pub fn show_glyph_editor(ui: &mut egui::Ui, state: &mut AppState) {
    let Some((glyph_set, _page)) = state.selected_context() else {
        ui.weak("No glyph selected.");
        return;
    };
    let size = glyph_set.glyph_size;
    let code = state.selection.code;
    let name = glyph_set.name.clone();

    // Header: what is being edited.
    ui.horizontal(|ui| {
        ui.strong(&name);
        ui.separator();
        ui.monospace(format!("code {code:#04X}"));
        if let Some(label) = state.selected_label() {
            ui.label(label.to_string());
        }
    });
    ui.separator();

    guides_section(ui, state);

    // Body: the painted matrix fills the remaining space and takes pointer input.
    let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
    let geom = MatrixGeometry::fit(rect, size);

    handle_input(state, &geom, &response);

    let painter = ui.painter_at(rect);
    let matrix = geom.matrix_rect();

    // Cells: matrix background, then committed on-pixels.
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

    // Live tentative stroke, drawn over the committed pixels before commit.
    if let Some(active) = state.active_stroke() {
        let color = if active.paint_on {
            TENTATIVE_ON
        } else {
            TENTATIVE_OFF
        };
        for &(x, y) in active.cells() {
            painter.rect_filled(geom.cell_rect(x, y), CornerRadius::ZERO, color);
        }
    }

    // Grid lines around/between cells (spec/12 §12.3).
    if let Some(color) = match state.grid {
        GridLevel::Off => None,
        GridLevel::Subtle => Some(GRID_SUBTLE),
        GridLevel::Strong => Some(GRID_STRONG),
    } {
        let grid_stroke = EguiStroke::new(1.0, color);
        for i in 0..=size.width {
            let x = geom.origin.x + i as f32 * geom.cell_size;
            painter.vline(x, Rangef::new(matrix.top(), matrix.bottom()), grid_stroke);
        }
        for j in 0..=size.height {
            let y = geom.origin.y + j as f32 * geom.cell_size;
            painter.hline(Rangef::new(matrix.left(), matrix.right()), y, grid_stroke);
        }
    }

    // Page guides, drawn on the grid lines over the matrix (spec/12 §12.6).
    let guide_stroke = EguiStroke::new(2.0, GUIDE);
    if let Some((_, page)) = state.selected_context() {
        for guide in &page.guides {
            if !guide.visible {
                continue;
            }
            let (a, b) = geom.guide_line(guide.axis, guide.position);
            painter.line_segment([a, b], guide_stroke);
        }
    }

    // Hover: outline the cell and show its coordinate with a legible backdrop.
    if let Some((x, y)) = response.hover_pos().and_then(|pos| geom.cell_at(pos)) {
        painter.rect_stroke(
            geom.cell_rect(x, y),
            CornerRadius::ZERO,
            EguiStroke::new(2.0, HOVER),
            StrokeKind::Inside,
        );
        let anchor = matrix.left_top() + egui::vec2(4.0, 4.0);
        let text_rect = painter
            .text(
                anchor,
                Align2::LEFT_TOP,
                format!("{x}, {y}"),
                FontId::monospace(12.0),
                HOVER,
            )
            .expand(2.0);
        painter.rect_filled(
            text_rect,
            CornerRadius::same(2),
            Color32::from_black_alpha(160),
        );
        painter.text(
            anchor,
            Align2::LEFT_TOP,
            format!("{x}, {y}"),
            FontId::monospace(12.0),
            HOVER,
        );
    }
}

/// A collapsing "Guides" section for the selected page (spec/12 §12.6): add
/// horizontal/vertical guides, show/hide, edit position, and remove. Guide edits go
/// through `fontspace-ops` and are individually undoable. (Dragging guides on the
/// matrix and rename/lock/copy-to-pages are follow-ups.)
fn guides_section(ui: &mut egui::Ui, state: &mut AppState) {
    let mut add_axis: Option<GuideAxis> = None;
    let mut toggle: Option<(GuideId, bool)> = None;
    let mut moved: Option<(GuideId, i32)> = None;
    let mut remove: Option<GuideId> = None;

    egui::CollapsingHeader::new("Guides")
        .id_salt(ui.id().with("guides"))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                if ui.button("Add horizontal").clicked() {
                    add_axis = Some(GuideAxis::Horizontal);
                }
                if ui.button("Add vertical").clicked() {
                    add_axis = Some(GuideAxis::Vertical);
                }
            });

            let Some((_, page)) = state.selected_context() else {
                return;
            };
            if page.guides.is_empty() {
                ui.weak("No guides on this page.");
                return;
            }
            for guide in &page.guides {
                ui.horizontal(|ui| {
                    let mut visible = guide.visible;
                    if ui.checkbox(&mut visible, "").changed() {
                        toggle = Some((guide.id, visible));
                    }
                    ui.label(match guide.axis {
                        GuideAxis::Horizontal => "H",
                        GuideAxis::Vertical => "V",
                    });
                    let mut position = guide.position;
                    // NOTE: dragging records one MoveGuide per integer step (each is
                    // a no-op-free change), so a drag spans several undo entries;
                    // typing a value is one. Coalescing a drag into one entry is a
                    // follow-up (like the matrix-drag interaction).
                    if ui.add(egui::DragValue::new(&mut position)).changed() {
                        moved = Some((guide.id, position));
                    }
                    ui.label(&guide.name);
                    if ui.small_button("Remove").clicked() {
                        remove = Some(guide.id);
                    }
                });
            }
        });

    // Apply after the immutable borrow of the page ends. At most one fires per frame.
    if let Some(axis) = add_axis {
        state.add_guide(axis);
    } else if let Some((id, visible)) = toggle {
        state.set_guide_visible(id, visible);
    } else if let Some((id, position)) = moved {
        state.move_guide(id, position);
    } else if let Some(id) = remove {
        state.remove_guide(id);
    }
}

/// Translates pointer gestures into stroke edits (spec/12 §12.4). A press begins a
/// stroke whose mode is fixed by the first pixel; dragging extends it (interpolated
/// so no cell is skipped); release commits it as one `SetPixels` — one undo entry.
fn handle_input(state: &mut AppState, geom: &MatrixGeometry, response: &egui::Response) {
    // `is_pointer_button_down_on` stays true while the button that pressed on this
    // widget is held, even if the pointer wanders off — so a click and a drag are the
    // same gesture, and dragging outside the matrix simply adds no cells.
    if response.is_pointer_button_down_on() {
        if let Some(cell) = response
            .interact_pointer_pos()
            .and_then(|pos| geom.cell_at(pos))
        {
            if state.active_stroke().is_none() {
                state.begin_stroke(cell);
            } else {
                state.extend_stroke(cell);
            }
        }
    } else if state.active_stroke().is_some() {
        // Button released (or the gesture ended): commit whatever was painted.
        state.commit_stroke();
    }
}
