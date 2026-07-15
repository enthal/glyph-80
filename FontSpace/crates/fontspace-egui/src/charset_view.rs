//! The character-set view tile (spec/12 §12.7): an ordered table of the selected
//! glyph set's character set — ordinal, `code`, the rendered character where
//! printable, control-character notation, and label. Clicking a row selects that
//! code. Removing an entry shows its **cascade impact before applying** (spec/12
//! §12.7, spec/07 §7.8): how many glyphs would be deleted.
//!
//! The `code`→display mapping is a pure function tested outside the paint closure.

use fontspace_model::CharacterSet;

use crate::state::AppState;

/// Standard ASCII C0 control names, indexed by code `0x00..=0x1F`.
const C0_NAMES: [&str; 32] = [
    "NUL", "SOH", "STX", "ETX", "EOT", "ENQ", "ACK", "BEL", "BS", "HT", "LF", "VT", "FF", "CR",
    "SO", "SI", "DLE", "DC1", "DC2", "DC3", "DC4", "NAK", "SYN", "ETB", "CAN", "EM", "SUB", "ESC",
    "FS", "GS", "RS", "US",
];

/// How a `code` is shown in the table: its printable character (if any) and its
/// control-character notation (if any). A control code has a notation and no glyph;
/// a printable code has a glyph and no notation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodeDisplay {
    pub glyph: Option<char>,
    pub notation: Option<&'static str>,
}

/// The ASCII control notation for `code`, if it is a C0 control or DEL.
fn control_notation(code: u32) -> Option<&'static str> {
    match code {
        0x00..=0x1F => Some(C0_NAMES[code as usize]),
        0x7F => Some("DEL"),
        _ => None,
    }
}

/// Maps a `code` to its table display: the Unicode scalar as a character when it is
/// not a control character, plus a control notation for C0/DEL codes. Codes with
/// neither (C1 controls `0x80..=0x9F`, and non-scalar values) show a blank cell —
/// only the C0/DEL names are given here.
pub fn describe_code(code: u32) -> CodeDisplay {
    CodeDisplay {
        glyph: char::from_u32(code).filter(|c| !c.is_control()),
        notation: control_notation(code),
    }
}

/// A hover tooltip describing a `code`: its hex value, its printable character or
/// control notation, and its character-set label when present. Shared by the page
/// overview and text preview (spec/12 §12.8, §12.10).
pub fn glyph_tooltip(code: u32, character_set: Option<&CharacterSet>) -> String {
    let display = describe_code(code);
    let mut tooltip = format!("0x{code:04X}");
    if let Some(glyph) = display.glyph {
        tooltip.push_str(&format!("  {glyph}"));
    } else if let Some(notation) = display.notation {
        tooltip.push_str(&format!("  {notation}"));
    }
    let label = character_set
        .and_then(|cs| cs.entry(code))
        .map(|entry| entry.label.as_str())
        .unwrap_or_default();
    if !label.is_empty() {
        tooltip.push_str(&format!("\n{label}"));
    }
    tooltip
}

/// Renders the character-set view, applying row-selection and entry removal.
pub fn show_character_set(ui: &mut egui::Ui, state: &mut AppState) {
    let selected_code = state.selection().code;
    // The pending remove's impact is computed against its armed target on a clone,
    // so it is safe (and consistent with what confirm will delete) to read here.
    let pending = state.pending_remove_impact();

    let mut select: Option<u32> = None;
    let mut request_remove: Option<u32> = None;
    let mut confirm = false;
    let mut cancel = false;

    {
        let Some(character_set) = state.selected_character_set() else {
            ui.weak("No character set selected.");
            return;
        };

        ui.horizontal(|ui| {
            ui.strong(&character_set.name);
            ui.separator();
            ui.label(format!("{} entries", character_set.entries.len()));
        });
        ui.separator();

        // Pending-remove confirmation, showing the cascade impact before applying.
        if let Some((code, cascade)) = pending {
            ui.horizontal_wrapped(|ui| {
                let glyphs = if cascade == 1 { "glyph" } else { "glyphs" };
                ui.colored_label(
                    egui::Color32::from_rgb(230, 110, 90),
                    format!("Remove {code:#04X}? This deletes {cascade} {glyphs}."),
                );
                if ui.button("Remove").clicked() {
                    confirm = true;
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
            });
            ui.separator();
        }

        egui::ScrollArea::vertical()
            .id_salt(ui.id().with("charset_scroll"))
            .show(ui, |ui| {
                egui::Grid::new(ui.id().with("charset_grid"))
                    .striped(true)
                    .num_columns(5)
                    .show(ui, |ui| {
                        ui.strong("#");
                        ui.strong("code");
                        ui.strong("char");
                        ui.strong("label");
                        ui.label("");
                        ui.end_row();

                        for (ordinal, entry) in character_set.entries.iter().enumerate() {
                            let display = describe_code(entry.code);
                            ui.monospace(ordinal.to_string());
                            if ui
                                .selectable_label(
                                    entry.code == selected_code,
                                    format!("{:04X}", entry.code),
                                )
                                .clicked()
                            {
                                select = Some(entry.code);
                            }
                            match (display.glyph, display.notation) {
                                (Some(glyph), _) => {
                                    ui.monospace(glyph.to_string());
                                }
                                (None, Some(notation)) => {
                                    ui.weak(notation);
                                }
                                (None, None) => {
                                    ui.label("");
                                }
                            }
                            ui.label(&entry.label);
                            // Plain ASCII text: the egui_kittest harness font renders
                            // no non-ASCII symbols (they show as missing-glyph boxes).
                            if ui.small_button("Remove").clicked() {
                                request_remove = Some(entry.code);
                            }
                            ui.end_row();
                        }
                    });
            });
    }

    if confirm {
        state.confirm_remove();
    } else if cancel {
        state.cancel_remove();
    } else if let Some(code) = request_remove {
        state.request_remove(code);
    }
    if let Some(code) = select {
        state.select_code(code);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn printable_codes_show_a_glyph_no_notation() {
        let a = describe_code(0x41);
        assert_eq!(a.glyph, Some('A'));
        assert_eq!(a.notation, None);
        // Space is printable (not a control character).
        assert_eq!(describe_code(0x20).glyph, Some(' '));
    }

    #[test]
    fn glyph_tooltip_combines_hex_character_and_label() {
        use fontspace_model::{CharacterEntry, SequentialIdGen};
        let mut ids = SequentialIdGen::new();
        let mut cs = CharacterSet::new(&mut ids, "cs", "");
        cs.entries = vec![CharacterEntry {
            code: 0x41,
            label: "Latin A".to_string(),
        }];
        // Printable code with a label: hex, character, then the label on its own line.
        assert_eq!(glyph_tooltip(0x41, Some(&cs)), "0x0041  A\nLatin A");
        // Control code shows its notation instead of a character; absent entry → no label.
        assert_eq!(glyph_tooltip(0x00, Some(&cs)), "0x0000  NUL");
        // No character set at all → just hex + character.
        assert_eq!(glyph_tooltip(0x42, None), "0x0042  B");
    }

    #[test]
    fn control_codes_show_notation_no_glyph() {
        assert_eq!(
            describe_code(0x00),
            CodeDisplay {
                glyph: None,
                notation: Some("NUL")
            }
        );
        assert_eq!(describe_code(0x1B).notation, Some("ESC"));
        assert_eq!(describe_code(0x0A).notation, Some("LF"));
        assert_eq!(describe_code(0x7F).notation, Some("DEL"));
        assert!(describe_code(0x1B).glyph.is_none());
    }

    #[test]
    fn non_control_high_codes_have_a_glyph_and_no_notation() {
        // 0xE9 = 'é' (Latin-1), printable, not a control code.
        assert_eq!(describe_code(0xE9).glyph, Some('é'));
        assert_eq!(describe_code(0xE9).notation, None);
    }
}
