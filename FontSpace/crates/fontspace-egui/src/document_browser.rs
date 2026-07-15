//! The document browser tile (spec/12 §12.2): each open file as a tree of its
//! **Character Sets**, **Glyph Sets** (with their pages), and **Export
//! Configurations** — not every glyph as a node. Clicking a page selects it (updating
//! the editor and the other views).
//!
//! The tree model is a pure function tested outside the paint closure; the render only
//! walks it. Create/rename/duplicate/delete/reorder and drag-between-files arrive in
//! later slices.

use fontspace_model::{FontSpace, GlyphSetId, PageId};

use crate::state::AppState;

/// One glyph set in the browser: its id, name, and pages in document order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlyphSetNode {
    pub id: GlyphSetId,
    pub name: String,
    pub pages: Vec<PageNode>,
}

/// One page under a glyph set — the only selectable leaf in this slice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageNode {
    pub id: PageId,
    pub name: String,
}

/// The browser's ordered view of a document's objects (spec/12 §12.2). Character sets
/// and export configs are shown by name; glyph sets carry their pages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserModel {
    pub character_sets: Vec<String>,
    pub glyph_sets: Vec<GlyphSetNode>,
    pub export_configs: Vec<String>,
}

/// Builds the browser tree for `document` in document order (spec/12 §12.2).
pub fn browser_model(document: &FontSpace) -> BrowserModel {
    BrowserModel {
        character_sets: document
            .character_sets
            .iter()
            .map(|cs| cs.name.clone())
            .collect(),
        glyph_sets: document
            .glyph_sets
            .iter()
            .map(|gs| GlyphSetNode {
                id: gs.id,
                name: gs.name.clone(),
                pages: gs
                    .pages
                    .iter()
                    .map(|page| PageNode {
                        id: page.id,
                        name: page.name.clone(),
                    })
                    .collect(),
            })
            .collect(),
        export_configs: document
            .export_configs
            .iter()
            .map(|ec| ec.name.clone())
            .collect(),
    }
}

/// Renders the document browser and applies a page click to the selection.
pub fn show_document_browser(ui: &mut egui::Ui, state: &mut AppState) {
    let model = browser_model(state.document());
    let selection = state.selection();
    let document_name = state.document_name();
    let mut clicked_page: Option<(GlyphSetId, PageId)> = None;

    // `ui.id()` is seeded with this tile's id, so salting from it keeps every widget id
    // unique even when the browser is open in two tiles at once (spec/12 §12.1).
    egui::ScrollArea::vertical()
        .id_salt(ui.id().with("document_browser"))
        .show(ui, |ui| {
            ui.strong(&document_name);
            ui.separator();

            section(ui, "Character Sets", "character_sets", |ui| {
                names(ui, &model.character_sets);
            });

            section(ui, "Glyph Sets", "glyph_sets", |ui| {
                if model.glyph_sets.is_empty() {
                    ui.weak("(none)");
                }
                for glyph_set in &model.glyph_sets {
                    egui::CollapsingHeader::new(&glyph_set.name)
                        .id_salt(ui.id().with(glyph_set.id.as_uuid()))
                        .default_open(true)
                        .show(ui, |ui| {
                            if glyph_set.pages.is_empty() {
                                ui.weak("(no pages)");
                            }
                            for page in &glyph_set.pages {
                                let selected = selection.glyph_set_id == glyph_set.id
                                    && selection.page_id == page.id;
                                if ui.selectable_label(selected, &page.name).clicked() {
                                    clicked_page = Some((glyph_set.id, page.id));
                                }
                            }
                        });
                }
            });

            section(ui, "Export Configurations", "export_configs", |ui| {
                names(ui, &model.export_configs);
            });
        });

    if let Some((glyph_set_id, page_id)) = clicked_page {
        state.select_page(glyph_set_id, page_id);
    }
}

/// A default-open collapsing section, salted so two browser tiles don't collide.
fn section(ui: &mut egui::Ui, title: &str, key: &str, body: impl FnOnce(&mut egui::Ui)) {
    egui::CollapsingHeader::new(title)
        .id_salt(ui.id().with(key))
        .default_open(true)
        .show(ui, body);
}

/// Lists names, or a muted "(none)" when empty.
fn names(ui: &mut egui::Ui, names: &[String]) {
    if names.is_empty() {
        ui.weak("(none)");
        return;
    }
    for name in names {
        ui.label(name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fontspace_model::SequentialIdGen;

    #[test]
    fn model_lists_objects_in_document_order_with_pages() {
        let state = AppState::with_ids(Box::new(SequentialIdGen::new()));
        let model = browser_model(state.document());

        // The starter document: one character set, one glyph set ("Terminal 8x8")
        // with a single "Regular" page, no export configs.
        assert_eq!(model.character_sets, vec!["ASCII (demo)".to_string()]);
        assert_eq!(model.glyph_sets.len(), 1);
        assert_eq!(model.glyph_sets[0].name, "Terminal 8x8");
        assert_eq!(model.glyph_sets[0].pages.len(), 1);
        assert_eq!(model.glyph_sets[0].pages[0].name, "Regular");
        assert!(model.export_configs.is_empty());
    }

    #[test]
    fn clicking_a_page_node_target_matches_the_starter_selection() {
        // The starter selection already points at the one glyph set + page, so the
        // browser's page node carries exactly those ids (a click is a no-op reselect).
        let state = AppState::with_ids(Box::new(SequentialIdGen::new()));
        let model = browser_model(state.document());
        let node = &model.glyph_sets[0];
        assert_eq!(node.id, state.selection().glyph_set_id);
        assert_eq!(node.pages[0].id, state.selection().page_id);
    }
}
