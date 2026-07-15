//! The page overview tile (spec/12 §12.8): every glyph in the selected page as a
//! grid of thumbnails. Absent codes show blank; a glyph stored for a code with no
//! character-set entry is flagged **dangling**. Clicking a thumbnail selects that
//! code (updating the editor).
//!
//! The ordered entry list is a pure function tested outside the paint closure; the
//! thumbnail painting reuses the editor's [`MatrixGeometry`].

use egui::{Align2, Color32, CornerRadius, FontId, Rect, Sense, Stroke, Vec2};
use fontspace_model::{Bitmap, CharacterSet, GlyphPage, GlyphSize};

use crate::charset_view::glyph_tooltip;
use crate::glyph_paint::paint_bitmap;
use crate::state::AppState;

const THUMB: f32 = 40.0;
const LABEL_H: f32 = 16.0;
const SELECTED: Color32 = Color32::from_rgb(255, 200, 60);
const DANGLING: Color32 = Color32::from_rgb(230, 110, 90);
/// The drag-selected range highlight (spec/12 §12.8) — a cool tint so it reads apart
/// from the warm `SELECTED` outline on the active editor code.
const RANGE: Color32 = Color32::from_rgb(150, 190, 255);

/// The inclusive slice of `ordered` codes between `a` and `b` (in either drag order),
/// in display order (spec/12 §12.8). Empty when either endpoint is absent from the
/// list, so a stale anchor never yields a bogus range.
pub fn codes_between(ordered: &[u32], a: u32, b: u32) -> Vec<u32> {
    let ia = ordered.iter().position(|&code| code == a);
    let ib = ordered.iter().position(|&code| code == b);
    match (ia, ib) {
        (Some(i), Some(j)) => {
            let (lo, hi) = (i.min(j), i.max(j));
            ordered[lo..=hi].to_vec()
        }
        _ => Vec::new(),
    }
}

/// One overview cell: a `code`, whether it has a character-set entry, and whether a
/// glyph is stored for it on the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverviewEntry {
    pub code: u32,
    /// `false` marks a **dangling** glyph — stored on the page but absent from the
    /// character set (spec/04 tolerate-and-flag).
    pub in_character_set: bool,
    pub has_glyph: bool,
}

/// The ordered overview entries for a page (spec/12 §12.8): the character set's
/// entries in canonical order, followed by any dangling glyph codes (stored on the
/// page but not in the set), the latter sorted by code for a stable display.
pub fn overview_entries(
    character_set: Option<&CharacterSet>,
    page: &GlyphPage,
) -> Vec<OverviewEntry> {
    let mut entries = Vec::new();
    if let Some(character_set) = character_set {
        for entry in &character_set.entries {
            entries.push(OverviewEntry {
                code: entry.code,
                in_character_set: true,
                has_glyph: page.glyph_of_code(entry.code).is_some(),
            });
        }
    }
    let mut dangling: Vec<u32> = page
        .glyphs
        .iter()
        .map(|glyph| glyph.code)
        .filter(|code| character_set.is_none_or(|cs| !cs.contains_code(*code)))
        .collect();
    dangling.sort_unstable();
    for code in dangling {
        entries.push(OverviewEntry {
            code,
            in_character_set: false,
            has_glyph: true,
        });
    }
    entries
}

/// Renders the page overview, drives drag-select of a code range (spec/12 §12.8), and
/// applies a plain thumbnail click to the selection.
pub fn show_page_overview(ui: &mut egui::Ui, state: &mut AppState) {
    let selected_code = state.selection().code;
    let range: Vec<u32> = state.page_glyph_selection().to_vec();

    // Range action bar — shown only while a run is drag-selected, so the view is
    // unchanged until you drag (keeping the default snapshot intact). Copy exports the
    // run; Blank/Invert transform every stored glyph in it as one undo entry each.
    if !range.is_empty() {
        let n = range.len();
        let plural = if n == 1 { "" } else { "s" };
        ui.horizontal_wrapped(|ui| {
            ui.label(format!("{n} glyph{plural} selected"));
            if ui.button("Copy").clicked()
                && let Some(fragment) = state.copy_page_selection()
            {
                ui.ctx().copy_text(fragment);
                state.set_status(format!("Copied {n} glyph{plural} to clipboard"));
            }
            if ui.button("Blank").clicked() {
                state.blank_page_selection();
            }
            if ui.button("Invert").clicked() {
                state.invert_page_selection();
            }
            if ui.button("Clear").clicked() {
                state.clear_page_selection();
            }
        });
        ui.separator();
    }

    let mut clicked = None;
    let mut drag_started_on = None;
    // Each thumbnail's rect, so an ongoing drag can resolve the code under the pointer
    // (a drag stays captured by its origin widget, so neighbours never see it).
    let mut cells: Vec<(u32, Rect)> = Vec::new();
    let ordered: Vec<u32>;
    let pointer = ui.input(|i| i.pointer.interact_pos());
    let released = ui.input(|i| i.pointer.any_released());

    {
        let doc = state.document();
        let Some(glyph_set) = doc.glyph_set(state.selection().glyph_set_id) else {
            ui.weak("No glyph set selected.");
            return;
        };
        let Some(page) = glyph_set.page_of_id(state.selection().page_id) else {
            ui.weak("No page selected.");
            return;
        };
        let character_set = doc.character_set(glyph_set.character_set_id);
        let entries = overview_entries(character_set, page);
        let size = glyph_set.glyph_size;
        ordered = entries.iter().map(|entry| entry.code).collect();

        ui.horizontal(|ui| {
            ui.strong(&page.name);
            ui.separator();
            ui.label(format!("{} codes", entries.len()));
        });
        ui.separator();

        egui::ScrollArea::vertical()
            .id_salt(ui.id().with("page_overview"))
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for entry in &entries {
                        let bitmap = page.glyph_of_code(entry.code).map(|g| &g.bitmap);
                        let selected = entry.code == selected_code;
                        let in_range = range.contains(&entry.code);
                        let response = thumbnail(ui, entry, bitmap, size, selected, in_range)
                            .on_hover_text(glyph_tooltip(entry.code, character_set));
                        cells.push((entry.code, response.rect));
                        if response.clicked() {
                            clicked = Some(entry.code);
                        }
                        if response.drag_started() {
                            drag_started_on = Some(entry.code);
                        }
                    }
                });
            });
    }

    // Apply the resolved gesture to the selection state (after the doc borrow ends).
    if let Some(code) = drag_started_on {
        state.begin_page_selection(code);
    }
    if state.is_page_selecting() {
        if let (Some(anchor), Some(pos)) = (state.page_selection_anchor(), pointer)
            && let Some(&(code, _)) = cells.iter().find(|(_, rect)| rect.contains(pos))
        {
            let run = codes_between(&ordered, anchor, code);
            if !run.is_empty() {
                state.set_page_selection_range(run);
            }
        }
        if released {
            state.end_page_selection();
        }
    }
    if let Some(code) = clicked {
        state.select_code(code);
    }
}

/// Draws one thumbnail (glyph square + code label) and returns its click response.
fn thumbnail(
    ui: &mut egui::Ui,
    entry: &OverviewEntry,
    bitmap: Option<&Bitmap>,
    size: GlyphSize,
    selected: bool,
    in_range: bool,
) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(THUMB, THUMB + LABEL_H), Sense::click_and_drag());
    let painter = ui.painter_at(rect);

    // Glyph square at the top of the cell.
    let square = Rect::from_min_size(rect.min, Vec2::splat(THUMB));
    paint_bitmap(&painter, square, bitmap, size);

    // Code label below; dangling codes are tinted as a warning.
    let label_color = if entry.in_character_set {
        ui.visuals().text_color()
    } else {
        DANGLING
    };
    painter.text(
        Rect::from_min_size(
            egui::pos2(rect.min.x, rect.min.y + THUMB),
            Vec2::new(THUMB, LABEL_H),
        )
        .center(),
        Align2::CENTER_CENTER,
        format!("{:04X}", entry.code),
        FontId::monospace(11.0),
        label_color,
    );

    // Range-selection tint (drawn under the active-code outline so both can show on the
    // anchor cell), then the selection outline, then a hover cue.
    if in_range {
        painter.rect_filled(square, CornerRadius::ZERO, RANGE.linear_multiply(0.25));
        painter.rect_stroke(
            square,
            CornerRadius::ZERO,
            Stroke::new(1.0, RANGE),
            egui::StrokeKind::Inside,
        );
    }
    if selected {
        painter.rect_stroke(
            square,
            CornerRadius::ZERO,
            Stroke::new(2.0, SELECTED),
            egui::StrokeKind::Inside,
        );
    } else if !in_range && response.hovered() {
        painter.rect_stroke(
            square,
            CornerRadius::ZERO,
            Stroke::new(1.0, ui.visuals().weak_text_color()),
            egui::StrokeKind::Inside,
        );
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use fontspace_model::{CharacterEntry, Glyph, GlyphSize, SequentialIdGen};

    fn charset(codes: &[u32]) -> CharacterSet {
        let mut ids = SequentialIdGen::new();
        let mut cs = CharacterSet::new(&mut ids, "cs", "");
        cs.entries = codes
            .iter()
            .map(|&code| CharacterEntry {
                code,
                label: String::new(),
            })
            .collect();
        cs
    }

    fn page_with_glyphs(codes: &[u32]) -> GlyphPage {
        let mut ids = SequentialIdGen::new();
        let mut page = GlyphPage::new(&mut ids, "p", "");
        for &code in codes {
            page.glyphs.push(Glyph {
                code,
                bitmap: Bitmap::new_blank(GlyphSize::new(8, 8)),
            });
        }
        page
    }

    #[test]
    fn entries_follow_charset_order_and_flag_missing_glyphs() {
        let cs = charset(&[0x41, 0x42, 0x43]);
        let page = page_with_glyphs(&[0x42]); // only B is drawn
        let entries = overview_entries(Some(&cs), &page);
        assert_eq!(entries.len(), 3);
        assert_eq!(
            entries[0],
            OverviewEntry {
                code: 0x41,
                in_character_set: true,
                has_glyph: false
            }
        );
        assert_eq!(
            entries[1],
            OverviewEntry {
                code: 0x42,
                in_character_set: true,
                has_glyph: true
            }
        );
        assert!(!entries[2].has_glyph);
    }

    #[test]
    fn dangling_glyphs_are_appended_and_flagged() {
        let cs = charset(&[0x41]);
        let page = page_with_glyphs(&[0x41, 0x99, 0x80]); // 0x99, 0x80 have no entry
        let entries = overview_entries(Some(&cs), &page);
        // A (in set) first, then dangling codes sorted ascending.
        assert_eq!(entries[0].code, 0x41);
        assert_eq!(
            entries[1],
            OverviewEntry {
                code: 0x80,
                in_character_set: false,
                has_glyph: true
            }
        );
        assert_eq!(
            entries[2],
            OverviewEntry {
                code: 0x99,
                in_character_set: false,
                has_glyph: true
            }
        );
    }

    #[test]
    fn no_character_set_shows_only_dangling_glyphs() {
        let page = page_with_glyphs(&[0x41]);
        let entries = overview_entries(None, &page);
        assert_eq!(entries.len(), 1);
        assert!(!entries[0].in_character_set);
    }

    #[test]
    fn codes_between_spans_the_inclusive_run_in_display_order() {
        let ordered = [0x41, 0x42, 0x43, 0x44, 0x45];
        assert_eq!(codes_between(&ordered, 0x42, 0x44), vec![0x42, 0x43, 0x44]);
    }

    #[test]
    fn codes_between_is_order_independent() {
        let ordered = [0x41, 0x42, 0x43, 0x44];
        // Dragging right-to-left yields the same display-ordered run.
        assert_eq!(
            codes_between(&ordered, 0x44, 0x42),
            codes_between(&ordered, 0x42, 0x44)
        );
    }

    #[test]
    fn codes_between_single_endpoint_is_one_code() {
        let ordered = [0x41, 0x42, 0x43];
        assert_eq!(codes_between(&ordered, 0x42, 0x42), vec![0x42]);
    }

    #[test]
    fn codes_between_absent_endpoint_is_empty() {
        // A stale anchor (not on this page) must not produce a bogus range.
        let ordered = [0x41, 0x42, 0x43];
        assert!(codes_between(&ordered, 0x99, 0x42).is_empty());
    }
}
