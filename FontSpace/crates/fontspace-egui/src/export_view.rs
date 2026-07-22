//! The Export Configuration tile (spec/12 §12.11/§12.12, spec/10): a form over the
//! selected export config's high-level parameters — name, source glyph set, scan
//! direction, and code-bits — plus a live 1:1 validation summary and an
//! **Export to file** action that writes the raw ROM image (spec/13 §13.1).
//!
//! The view owns no export semantics (CLAUDE.md "the one architectural rule"): it edits
//! a working [`ExportConfigForm`] on [`AppState`] and, on **Apply**, has the state
//! rebuild the config from a `fontspace-export` scan preset and replace it as one undo
//! entry. Validation is `fontspace-export::validate_export`, rendered here.

use fontspace_export::ScanDirection;
use fontspace_model::GlyphSetId;

use crate::state::AppState;

/// Renders the Export Configuration view for `state`'s selected config, or guidance when
/// none is selected.
pub fn show_export_configuration(ui: &mut egui::Ui, state: &mut AppState) {
    let Some(_id) = state.selected_export_config() else {
        show_empty_guidance(ui);
        return;
    };

    // The source-glyph-set picker lists (id, name); snapshot it before borrowing the
    // form mutably so the combo can render without holding a document borrow.
    let glyph_sets: Vec<(GlyphSetId, String)> = state
        .document()
        .glyph_sets
        .iter()
        .map(|gs| (gs.id, gs.name.clone()))
        .collect();

    let mut apply = false;
    let mut revert = false;
    let dirty = state.export_form_is_dirty();

    // `export_form_mut` initializes the draft from the selected config on first view.
    let Some(form) = state.export_form_mut() else {
        show_empty_guidance(ui);
        return;
    };

    egui::ScrollArea::vertical()
        .id_salt(ui.id().with("export_configuration"))
        .show(ui, |ui| {
            ui.add_space(4.0);
            egui::Grid::new(ui.id().with("export_form_grid"))
                .num_columns(2)
                .spacing([8.0, 6.0])
                .show(ui, |ui| {
                    ui.label("Name");
                    ui.text_edit_singleline(&mut form.name);
                    ui.end_row();

                    ui.label("Source glyph set");
                    let selected = glyph_sets
                        .iter()
                        .find(|(id, _)| *id == form.glyph_set_id)
                        .map(|(_, name)| name.clone())
                        .unwrap_or_else(|| "—".to_string());
                    egui::ComboBox::from_id_salt(ui.id().with("export_source"))
                        .selected_text(selected)
                        .show_ui(ui, |ui| {
                            for (id, name) in &glyph_sets {
                                ui.selectable_value(&mut form.glyph_set_id, *id, name);
                            }
                        });
                    ui.end_row();

                    ui.label("Scan");
                    egui::ComboBox::from_id_salt(ui.id().with("export_scan"))
                        .selected_text(scan_label(form.scan))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut form.scan,
                                ScanDirection::Row,
                                scan_label(ScanDirection::Row),
                            );
                            ui.selectable_value(
                                &mut form.scan,
                                ScanDirection::Column,
                                scan_label(ScanDirection::Column),
                            );
                        });
                    ui.end_row();

                    ui.label("Code bits");
                    ui.add(egui::DragValue::new(&mut form.code_bits).range(1..=24));
                    ui.end_row();

                    // Output size is a power of two, entered as its exponent (the target
                    // EEPROM's address-bit count), with the humanized byte size beside it.
                    // The checkbox toggles padding on/off (off = the natural image size).
                    ui.label("Output size");
                    ui.horizontal(|ui| {
                        let mut pad = form.output_address_bits.is_some();
                        if ui.checkbox(&mut pad, "pad to 2^").changed() {
                            form.output_address_bits = pad.then_some(16); // default 64 KiB
                        }
                        if let Some(bits) = &mut form.output_address_bits {
                            ui.add(egui::DragValue::new(bits).range(1..=32));
                            ui.label(format!("= {}", humanized_size(*bits)));
                        } else {
                            ui.weak("natural (image only)");
                        }
                    });
                    ui.end_row();

                    ui.label("Fill byte");
                    ui.add(
                        egui::DragValue::new(&mut form.fill_byte)
                            .range(0..=255)
                            .hexadecimal(2, false, true)
                            .prefix("0x"),
                    );
                    ui.end_row();
                });

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.add_enabled(dirty, egui::Button::new("Apply")).clicked() {
                    apply = true;
                }
                if ui.add_enabled(dirty, egui::Button::new("Revert")).clicked() {
                    revert = true;
                }
                if dirty {
                    ui.label(egui::RichText::new("unapplied changes").weak().italics());
                }
            });

            ui.separator();
            // Pages are always the source's pages in order in this slice (page-subset
            // editing is a follow-up); surface that so the shape isn't a mystery.
            ui.label(
                egui::RichText::new("Pages: all of the source glyph set, in order")
                    .weak()
                    .small(),
            );
        });

    // Buttons only set flags; act after the borrow of `form`/`ui` is released.
    if apply {
        state.apply_export_form();
    } else if revert {
        state.reset_export_form();
    }

    ui.separator();
    show_validation(ui, state);
}

/// The live 1:1 validation summary of the selected (saved) config, or its diagnostic,
/// followed by the "Export to file" action (enabled only when the config is valid).
fn show_validation(ui: &mut egui::Ui, state: &mut AppState) {
    ui.strong("Validation");
    ui.add_space(2.0);
    let valid = match state.validate_selected_export() {
        Some(Ok(summary)) => {
            ui.label(egui::RichText::new(summary.to_string()).monospace());
            true
        }
        Some(Err(err)) => {
            ui.colored_label(ui.visuals().error_fg_color, err.to_string());
            false
        }
        None => {
            ui.weak("(nothing to validate)");
            false
        }
    };

    ui.add_space(6.0);
    if ui
        .add_enabled(valid, egui::Button::new("Export to file…"))
        .on_hover_text("Write the raw ROM image to a .bin file")
        .clicked()
    {
        export_to_file(state);
    }
}

/// Renders the selected config to bytes and writes them to a user-chosen `.bin` (spec/10
/// §10.9, spec/13 §13.1). The native save dialog runs only on the click, never in tests.
fn export_to_file(state: &mut AppState) {
    let bytes = match state.export_selected_bytes() {
        Ok(bytes) => bytes,
        Err(message) => {
            state.set_error(message);
            return;
        }
    };
    let Some(path) = rfd::FileDialog::new()
        .add_filter("ROM image", &["bin"])
        .set_file_name("rom.bin")
        .save_file()
    else {
        return; // dialog cancelled
    };
    match std::fs::write(&path, &bytes) {
        Ok(()) => state.set_status(format!("Wrote {} bytes to {}", bytes.len(), path.display())),
        Err(err) => state.set_error(format!("Export failed: {err}")),
    }
}

/// The "no config selected" guidance (spec/12 §12.12).
fn show_empty_guidance(ui: &mut egui::Ui) {
    ui.vertical(|ui| {
        ui.add_space(8.0);
        ui.heading("Export Configuration");
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(
                "No export configuration selected.\n\
                 Insert ▸ Export Configuration to create one, or pick one under \
                 “Export Configurations” in the Documents browser.",
            )
            .weak(),
        );
    });
}

/// A humanized byte size for `2^bits`: e.g. `10 → "1 KiB"`, `16 → "64 KiB"`, `20 → "1 MiB"`.
fn humanized_size(bits: u8) -> String {
    // Use the largest binary unit no bigger than the size; the count is then `2^(bits -
    // unit_bits)`, a whole power-of-two number of B / KiB / MiB / GiB.
    const UNITS: [(&str, u8); 4] = [("GiB", 30), ("MiB", 20), ("KiB", 10), ("B", 0)];
    for (unit, unit_bits) in UNITS {
        if bits >= unit_bits {
            return format!("{} {unit}", 1u64 << (bits - unit_bits));
        }
    }
    // Unreachable — the ("B", 0) unit always matches — but keeps the function total.
    format!("{} B", 1u64 << bits)
}

/// The human label for a scan direction.
fn scan_label(scan: ScanDirection) -> &'static str {
    match scan {
        ScanDirection::Row => "Row (columns on data bits)",
        ScanDirection::Column => "Column (rows on data bits)",
    }
}

#[cfg(test)]
mod tests {
    use super::humanized_size;

    #[test]
    fn humanized_size_picks_whole_binary_units() {
        assert_eq!(humanized_size(0), "1 B");
        assert_eq!(humanized_size(9), "512 B");
        assert_eq!(humanized_size(10), "1 KiB");
        assert_eq!(humanized_size(16), "64 KiB");
        assert_eq!(humanized_size(20), "1 MiB");
        assert_eq!(humanized_size(30), "1 GiB");
    }
}
