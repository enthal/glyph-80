//! The text-preview tile (spec/12 §12.10): editable sample text rendered with the
//! selected page's glyphs. Each input character maps to a `code` (its Unicode
//! scalar); characters with no character-set entry are ignored, and an entry with no
//! glyph renders blank — the same rule as `render_text_string` (spec/09 §9.2.1).
//!
//! The `text`→`codes` mapping is a pure function tested outside the paint closure;
//! glyph painting reuses the editor's [`MatrixGeometry`].

use egui::{Color32, Rangef, Response, Sense, Stroke, Vec2};
use fontspace_model::{Bitmap, CharacterSet, GlyphSize};

use crate::charset_view::glyph_tooltip;
use crate::glyph_paint::paint_bitmap;
use crate::state::AppState;

/// Pixels per glyph pixel in the preview.
const SCALE: f32 = 4.0;
/// The 1px divider drawn between glyph cells when enabled (spec/12 §12.10).
const DIVIDER: Color32 = Color32::from_rgb(80, 160, 240);

/// The ordered `code`s the preview renders: each input character's scalar, keeping
/// only those with a character-set entry (spec/09 §9.2.1). Repeats are preserved.
pub fn preview_codes(text: &str, character_set: Option<&CharacterSet>) -> Vec<u32> {
    text.chars()
        .map(|c| c as u32)
        .filter(|&code| character_set.is_some_and(|cs| cs.contains_code(code)))
        .collect()
}

/// Renders the text-preview view. The sample text is UI state edited in place.
pub fn show_text_preview(ui: &mut egui::Ui, state: &mut AppState) {
    ui.horizontal(|ui| {
        ui.label("Sample:");
        ui.add(
            egui::TextEdit::singleline(&mut state.preview_text)
                .hint_text("type sample text")
                .desired_width(f32::INFINITY),
        );
    });
    ui.checkbox(&mut state.preview_dividers, "Dividers");
    ui.separator();

    // Snapshot what the render needs; the header's `&mut state` writes are done.
    let dividers = state.preview_dividers;
    let Some((glyph_set, page)) = state.selected_context() else {
        ui.weak("No page selected.");
        return;
    };
    let size = glyph_set.glyph_size;
    let character_set = state.document().character_set(glyph_set.character_set_id);
    let codes = preview_codes(&state.preview_text, character_set);

    if codes.is_empty() {
        ui.weak("No renderable characters (only codes in the character set show).");
        return;
    }

    // A click selects that glyph (like the page overview); applied after the render so
    // the immutable borrow of `state` (via `page`/`character_set`) has ended.
    let mut clicked = None;
    egui::ScrollArea::vertical()
        .id_salt(ui.id().with("text_preview"))
        .show(ui, |ui| {
            // Glyph cells sit flush — no inter-cell or inter-row spacing.
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::ZERO;
                for &code in &codes {
                    let bitmap = page.glyph_of_code(code).map(|glyph| &glyph.bitmap);
                    let response = paint_glyph(ui, bitmap, size, dividers)
                        .on_hover_text(glyph_tooltip(code, character_set));
                    if response.clicked() {
                        clicked = Some(code);
                    }
                }
            });
        });

    if let Some(code) = clicked {
        state.select_code(code);
    }
}

/// Paints one glyph cell at the preview scale, returning its click response. With
/// `dividers`, draws a 1px line on the cell's right and bottom edges so flush cells
/// read as a grid (spec/12 §12.10).
fn paint_glyph(
    ui: &mut egui::Ui,
    bitmap: Option<&Bitmap>,
    size: GlyphSize,
    dividers: bool,
) -> Response {
    let extent = Vec2::new(size.width as f32 * SCALE, size.height as f32 * SCALE);
    let (rect, response) = ui.allocate_exact_size(extent, Sense::click());
    let painter = ui.painter_at(rect);
    paint_bitmap(&painter, rect, bitmap, size);
    if dividers {
        let stroke = Stroke::new(1.0, DIVIDER);
        painter.vline(
            rect.right() - 0.5,
            Rangef::new(rect.top(), rect.bottom()),
            stroke,
        );
        painter.hline(
            Rangef::new(rect.left(), rect.right()),
            rect.bottom() - 0.5,
            stroke,
        );
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use fontspace_model::{CharacterEntry, SequentialIdGen};

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

    #[test]
    fn preview_codes_map_chars_keep_repeats_and_drop_unknown() {
        let cs = charset(&[0x41, 0x42]);
        // 'x' (0x78) has no entry → dropped; 'A'/'B' kept, repeats preserved.
        assert_eq!(preview_codes("AxAB", Some(&cs)), vec![0x41, 0x41, 0x42]);
    }

    #[test]
    fn preview_codes_empty_without_character_set() {
        assert_eq!(preview_codes("AB", None), Vec::<u32>::new());
    }
}
