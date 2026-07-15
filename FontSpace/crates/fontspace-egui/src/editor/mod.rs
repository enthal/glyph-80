//! The glyph editor tile: a custom-painted view of the selected glyph's pixel
//! matrix, with grid lines, page guides, a hover-coordinate readout, and
//! first-pixel-determines-stroke pixel editing (spec/12 §12.3–§12.4). It is a
//! painted widget, **not** a matrix of `Button`s.
//!
//! All layout and stroke math lives in [`geometry`] and [`stroke`] and is unit-tested
//! outside this paint/input code.

pub mod geometry;
pub mod region;
pub mod stroke;

use egui::{
    Align2, Color32, CornerRadius, FontId, Rangef, Rect, Sense, Stroke as EguiStroke, StrokeKind,
};
use fontspace_model::{GuideAxis, GuideId};

use crate::glyph_paint::paint_bitmap;
use crate::state::AppState;
use geometry::{GridLevel, MatrixGeometry};
use region::FlipDir;

const GRID_SUBTLE: Color32 = Color32::from_gray(64);
const GRID_STRONG: Color32 = Color32::from_gray(110);
const HOVER: Color32 = Color32::from_rgb(255, 200, 60);
/// The rectangular pixel-selection marquee (spec/12 §12.4).
const SELECTION: Color32 = Color32::from_rgb(240, 240, 255);
/// Tentative paint (a stroke in progress, before commit).
const TENTATIVE_ON: Color32 = Color32::from_rgb(180, 210, 120);
const TENTATIVE_OFF: Color32 = Color32::from_rgb(70, 60, 40);

/// A stable, distinct color for a guide, derived from its id (spec/12 §12.6). Keying
/// on the id means a guide keeps its color across renders and reorders. Hues are
/// scattered around the wheel by the golden-ratio conjugate so even sequential ids
/// (and neighbouring guides) land far apart; saturation/value are fixed for legible,
/// bright lines over the dark matrix.
pub fn guide_color(id: GuideId) -> Color32 {
    // Reduce the id to a bounded integer, then step the hue by the golden ratio so
    // successive values fall at 0.618, 0.236, 0.854, … around the wheel.
    let n = (id.as_uuid().as_u128() % 4096) as f64;
    let hue = (n * 0.618_033_988_749_895).fract() as f32;
    egui::ecolor::Hsva::new(hue, 0.65, 1.0, 1.0).into()
}

/// Renders the glyph editor for the current selection into `ui`, handling pointer
/// editing. Mutates `state` (applies a committed stroke, tracks the in-progress one).
pub fn show_glyph_editor(ui: &mut egui::Ui, state: &mut AppState) {
    let Some((glyph_set, _page)) = state.selected_context() else {
        ui.weak("No glyph selected.");
        return;
    };
    let size = glyph_set.glyph_size;
    let code = state.selection().code;
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
    region_toolbar(ui, state);

    // Body: the painted matrix fills the remaining space and takes pointer input.
    let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
    let geom = MatrixGeometry::fit(rect, size);

    handle_input(state, &geom, &response);

    let painter = ui.painter_at(rect);
    let matrix = geom.matrix_rect();

    // Cells: matrix background + committed on-pixels (shared with the other views).
    paint_bitmap(&painter, matrix, state.selected_bitmap(), size);

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

    // Page guides, drawn on the grid lines over the matrix, each in its own stable
    // color so they are told apart at a glance (spec/12 §12.6).
    if let Some((_, page)) = state.selected_context() {
        for guide in &page.guides {
            if !guide.visible {
                continue;
            }
            let (a, b) = geom.guide_line(guide.axis, guide.position);
            painter.line_segment([a, b], EguiStroke::new(2.0, guide_color(guide.id)));
        }
    }

    // Pixel-region marquee, drawn as an outline enclosing the selected cells.
    if let Some(rect) = state.pixel_selection() {
        let sel = Rect::from_min_max(
            geom.cell_rect(rect.x0, rect.y0).min,
            geom.cell_rect(rect.x1, rect.y1).max,
        );
        painter.rect_stroke(
            sel,
            CornerRadius::ZERO,
            EguiStroke::new(2.0, SELECTION),
            StrokeKind::Inside,
        );
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
/// horizontal/vertical guides, show/hide, rename, edit position, and remove. Guide
/// edits go through `fontspace-ops` and are individually undoable. (Dragging guides on
/// the matrix and lock/copy-to-pages are follow-ups.)
fn guides_section(ui: &mut egui::Ui, state: &mut AppState) {
    let mut add_axis: Option<GuideAxis> = None;
    let mut toggle: Option<(GuideId, bool)> = None;
    let mut moved: Option<(GuideId, i32)> = None;
    let mut renamed: Option<(GuideId, String)> = None;
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
                    // A swatch matching the guide's line color on the matrix.
                    let (swatch, _) =
                        ui.allocate_exact_size(egui::vec2(12.0, 12.0), Sense::hover());
                    ui.painter()
                        .rect_filled(swatch, CornerRadius::same(2), guide_color(guide.id));
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
                    // Editable name: commit on losing focus (Enter/Tab/click-away) so a
                    // rename is one undo entry, not one per keystroke. The id is salted
                    // with the guide id so the fields never share a widget id.
                    let mut name = guide.name.clone();
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut name)
                            .desired_width(90.0)
                            .id_salt(guide.id),
                    );
                    if response.lost_focus() && name != guide.name {
                        renamed = Some((guide.id, name));
                    }
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
    } else if let Some((id, name)) = renamed {
        state.rename_guide(id, name);
    } else if let Some(id) = remove {
        state.remove_guide(id);
    }
}

/// Controls for the pixel-region marquee — shown only while a selection exists, so
/// the editor is unchanged until you Shift+drag (spec/12 §12.4). Flip mirrors the
/// region in place (the "reverse"); Clear drops the marquee.
fn region_toolbar(ui: &mut egui::Ui, state: &mut AppState) {
    let Some(rect) = state.pixel_selection() else {
        return;
    };
    ui.horizontal(|ui| {
        ui.label(format!("Selection {}×{}", rect.width(), rect.height()));
        if ui.button("Flip H").clicked() {
            state.flip_selection(FlipDir::LeftRight);
        }
        if ui.button("Flip V").clicked() {
            state.flip_selection(FlipDir::TopBottom);
        }
        if ui.button("Copy").clicked() {
            state.copy_selection();
        }
        // Paste stamps the copied region with its top-left at the marquee's top-left.
        if ui
            .add_enabled(state.has_region_clipboard(), egui::Button::new("Paste"))
            .clicked()
        {
            state.paste_region();
        }
        if ui.button("Clear").clicked() {
            state.clear_selection();
        }
    });
    ui.separator();
}

/// Translates pointer gestures into stroke edits or a region marquee (spec/12 §12.4).
/// **Shift+drag selects** a rectangle of cells; a plain drag paints, its mode fixed by
/// the first pixel and interpolated so no cell is skipped. The gesture's kind is fixed
/// at press (whichever is already in progress continues); release commits the stroke
/// or ends the selection drag (the marquee persists).
fn handle_input(state: &mut AppState, geom: &MatrixGeometry, response: &egui::Response) {
    // `is_pointer_button_down_on` stays true while the button that pressed on this
    // widget is held, even if the pointer wanders off — so a click and a drag are the
    // same gesture, and dragging outside the matrix simply adds no cells.
    if response.is_pointer_button_down_on() {
        let shift = response.ctx.input(|i| i.modifiers.shift);
        if let Some(cell) = response
            .interact_pointer_pos()
            .and_then(|pos| geom.cell_at(pos))
        {
            if state.is_selecting() {
                state.extend_selection(cell); // continue an in-progress marquee
            } else if state.active_stroke().is_some() {
                state.extend_stroke(cell);
            } else if shift {
                state.begin_selection(cell); // Shift held at press → marquee
            } else {
                state.begin_stroke(cell);
            }
        }
    } else {
        // Button released (or the gesture ended): finish whichever gesture was active.
        if state.active_stroke().is_some() {
            state.commit_stroke();
        }
        if state.is_selecting() {
            state.end_selection();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fontspace_model::SequentialIdGen;

    #[test]
    fn guide_color_is_deterministic_and_distinct_per_id() {
        let mut ids = SequentialIdGen::new();
        let a = GuideId::new(&mut ids);
        let b = GuideId::new(&mut ids);
        let c = GuideId::new(&mut ids);
        // Same id → same color every time.
        assert_eq!(guide_color(a), guide_color(a));
        // Consecutive (sequential) ids land on visibly different hues — the
        // golden-ratio scatter keeps even adjacent guides apart.
        assert_ne!(guide_color(a), guide_color(b));
        assert_ne!(guide_color(b), guide_color(c));
        assert_ne!(guide_color(a), guide_color(c));
    }
}
