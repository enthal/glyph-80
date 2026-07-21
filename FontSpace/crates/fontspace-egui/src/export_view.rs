//! The Export Configuration tile (spec/12 §12.11/§12.12, spec/10): a form over the
//! selected export config's high-level parameters — name, source glyph set, scan
//! direction, and code-bits — plus a live 1:1 validation summary.
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

/// The live 1:1 validation summary of the selected (saved) config, or its diagnostic.
fn show_validation(ui: &mut egui::Ui, state: &AppState) {
    ui.strong("Validation");
    ui.add_space(2.0);
    match state.validate_selected_export() {
        Some(Ok(summary)) => {
            ui.label(egui::RichText::new(summary.to_string()).monospace());
        }
        Some(Err(err)) => {
            ui.colored_label(ui.visuals().error_fg_color, err.to_string());
        }
        None => {
            ui.weak("(nothing to validate)");
        }
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

/// The human label for a scan direction.
fn scan_label(scan: ScanDirection) -> &'static str {
    match scan {
        ScanDirection::Row => "Row (columns on data bits)",
        ScanDirection::Column => "Column (rows on data bits)",
    }
}
