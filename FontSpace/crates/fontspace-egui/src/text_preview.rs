//! The text-preview tile (spec/12 §12.10): editable sample text rendered with the
//! selected page's glyphs. Each input character maps to a `code` (its Unicode
//! scalar); characters with no character-set entry are ignored, and an entry with no
//! glyph renders blank — the same rule as `render_text_string` (spec/09 §9.2.1).
//!
//! The `text`→`codes` mapping is a pure function tested outside the paint closure;
//! glyph painting reuses the editor's [`MatrixGeometry`].

use egui::{Sense, Vec2};
use fontspace_model::{Bitmap, CharacterSet, GlyphSize};

use crate::glyph_paint::paint_bitmap;
use crate::state::AppState;

/// Pixels per glyph pixel in the preview.
const SCALE: f32 = 4.0;

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
    ui.separator();

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

    egui::ScrollArea::vertical()
        .id_salt(ui.id().with("text_preview"))
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                for code in codes {
                    let bitmap = page.glyph_of_code(code).map(|glyph| &glyph.bitmap);
                    paint_glyph(ui, bitmap, size);
                }
            });
        });
}

/// Paints one glyph cell at the preview scale.
fn paint_glyph(ui: &mut egui::Ui, bitmap: Option<&Bitmap>, size: GlyphSize) {
    let extent = Vec2::new(size.width as f32 * SCALE, size.height as f32 * SCALE);
    let (rect, _response) = ui.allocate_exact_size(extent, Sense::hover());
    paint_bitmap(&ui.painter_at(rect), rect, bitmap, size);
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
