//! The document browser tile (spec/12 §12.2): each open file as a tree of its
//! **Character Sets**, **Glyph Sets** (with their pages), and **Export
//! Configurations** — not every glyph as a node. Clicking a page selects it (updating
//! the editor and the other views).
//!
//! The tree model is a pure function tested outside the paint closure; the render only
//! walks it. Create/rename/duplicate/delete/reorder and drag-between-files arrive in
//! later slices.

use fontspace_model::{ExportConfigId, FontSpace, GlyphSetId, GlyphSize, PageId};

use crate::state::{AppState, RenameTarget};
use crate::workspace::DocumentId;

/// One glyph set in the browser: its id, name, geometry, and pages in document order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlyphSetNode {
    pub id: GlyphSetId,
    pub name: String,
    pub glyph_size: GlyphSize,
    pub pages: Vec<PageNode>,
}

/// One page under a glyph set — the only selectable leaf in this slice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageNode {
    pub id: PageId,
    pub name: String,
}

/// One export config in the browser: its id and name. Selectable — clicking it points
/// the Export Configuration view at it (spec/12 §12.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportConfigNode {
    pub id: ExportConfigId,
    pub name: String,
}

/// The browser's ordered view of a document's objects (spec/12 §12.2). Character sets
/// are shown by name; glyph sets carry their pages; export configs are selectable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserModel {
    pub character_sets: Vec<String>,
    pub glyph_sets: Vec<GlyphSetNode>,
    pub export_configs: Vec<ExportConfigNode>,
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
                glyph_size: gs.glyph_size,
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
            .map(|ec| ExportConfigNode {
                id: ec.id,
                name: ec.name.clone(),
            })
            .collect(),
    }
}

/// One open document prepared for rendering: its id, display name, active flag, and
/// object tree. Built outside the paint closure so no document borrow is held while
/// the trailing `&mut state` switch/select runs.
struct DocumentEntry {
    id: DocumentId,
    name: String,
    active: bool,
    model: BrowserModel,
}

/// A create/rename/duplicate/delete/move action chosen from a right-click context menu
/// (spec/12 §12.2). Collected during render (which holds a document borrow) and applied
/// against `&mut state` afterwards. Only offered on the **active** document, so each
/// applies to the active document's content. Names are resolved at menu time.
enum BrowserAction {
    AddPage(GlyphSetId, String),
    RenameGlyphSet(GlyphSetId),
    DuplicateGlyphSet(GlyphSetId, String),
    RemoveGlyphSet(GlyphSetId),
    RenamePage(GlyphSetId, PageId),
    DuplicatePage(GlyphSetId, PageId, String),
    RemovePage(GlyphSetId, PageId),
    MovePage {
        from: GlyphSetId,
        page: PageId,
        to: GlyphSetId,
    },
}

/// Renders the document browser — every open file as a collapsing tree — and applies a
/// page click: switch to that file (if it isn't active) and select the page.
pub fn show_document_browser(ui: &mut egui::Ui, state: &mut AppState) {
    let selection = state.selection();
    let selected_export = state.selected_export_config();
    let documents: Vec<DocumentEntry> = state
        .open_documents()
        .map(|(document, active)| DocumentEntry {
            id: document.id,
            name: document.display_name(),
            active,
            model: browser_model(&document.content),
        })
        .collect();

    let mut clicked: Option<(DocumentId, GlyphSetId, PageId)> = None;
    let mut clicked_export: Option<(DocumentId, ExportConfigId)> = None;
    let mut actions: Vec<BrowserAction> = Vec::new();
    // `ui.id()` is seeded with this tile's id, so salting from it keeps every widget id
    // unique even when the browser is open in two tiles at once (spec/12 §12.1).
    egui::ScrollArea::vertical()
        .id_salt(ui.id().with("document_browser"))
        .show(ui, |ui| {
            for document in &documents {
                let title = if document.active {
                    format!("{}  (active)", document.name)
                } else {
                    document.name.clone()
                };
                egui::CollapsingHeader::new(title)
                    .id_salt(ui.id().with(document.id.0))
                    .default_open(document.active)
                    .show(ui, |ui| {
                        show_document_tree(
                            ui,
                            document,
                            selection,
                            selected_export,
                            &mut clicked,
                            &mut clicked_export,
                            &mut actions,
                        );
                    });
            }
        });

    if let Some((document_id, glyph_set_id, page_id)) = clicked {
        state.switch_to(document_id);
        state.select_page(glyph_set_id, page_id);
    }
    if let Some((document_id, export_config_id)) = clicked_export {
        state.switch_to(document_id);
        state.select_export_config(export_config_id);
    }
    // Context-menu actions were only offered on the active document, so they apply to the
    // active document's content (spec/12 §12.2). Each is one undo entry.
    for action in actions {
        match action {
            BrowserAction::AddPage(glyph_set, name) => state.add_page(glyph_set, name),
            BrowserAction::RenameGlyphSet(glyph_set) => {
                state.begin_rename(RenameTarget::GlyphSet(glyph_set))
            }
            BrowserAction::DuplicateGlyphSet(glyph_set, name) => {
                state.duplicate_glyph_set(glyph_set, name)
            }
            BrowserAction::RemoveGlyphSet(glyph_set) => state.remove_glyph_set(glyph_set),
            BrowserAction::RenamePage(glyph_set, page) => {
                state.begin_rename(RenameTarget::Page(glyph_set, page))
            }
            BrowserAction::DuplicatePage(glyph_set, page, name) => {
                state.duplicate_page(glyph_set, page, name)
            }
            BrowserAction::RemovePage(glyph_set, page) => state.remove_page(glyph_set, page),
            BrowserAction::MovePage { from, page, to } => state.move_page(from, page, to),
        }
    }
}

/// Renders one document's object tree (spec/12 §12.2); page clicks are recorded into
/// `clicked` with the document's id. Only the active document highlights the current
/// selection.
fn show_document_tree(
    ui: &mut egui::Ui,
    document: &DocumentEntry,
    selection: crate::state::Selection,
    selected_export: Option<ExportConfigId>,
    clicked: &mut Option<(DocumentId, GlyphSetId, PageId)>,
    clicked_export: &mut Option<(DocumentId, ExportConfigId)>,
    actions: &mut Vec<BrowserAction>,
) {
    let model = &document.model;
    section(ui, "Character Sets", "character_sets", |ui| {
        names(ui, &model.character_sets);
    });

    section(ui, "Glyph Sets", "glyph_sets", |ui| {
        if model.glyph_sets.is_empty() {
            ui.weak("(none)");
        }
        for glyph_set in &model.glyph_sets {
            let header = egui::CollapsingHeader::new(&glyph_set.name)
                .id_salt(ui.id().with(glyph_set.id.as_uuid()))
                .default_open(true)
                .show(ui, |ui| {
                    if glyph_set.pages.is_empty() {
                        ui.weak("(no pages)");
                    }
                    for page in &glyph_set.pages {
                        let selected = document.active
                            && selection.glyph_set_id == glyph_set.id
                            && selection.page_id == page.id;
                        let response = ui.selectable_label(selected, &page.name);
                        if response.clicked() {
                            *clicked = Some((document.id, glyph_set.id, page.id));
                        }
                        // Page context menu (active document only): rename / duplicate /
                        // move to a same-geometry glyph set / delete.
                        if document.active {
                            response.context_menu(|ui| {
                                page_context_menu(ui, model, glyph_set, page, actions);
                            });
                        }
                    }
                });
            // Glyph-set context menu (active document only): add page / rename / duplicate
            // / delete.
            if document.active {
                header.header_response.context_menu(|ui| {
                    if ui.button("Add page").clicked() {
                        actions.push(BrowserAction::AddPage(
                            glyph_set.id,
                            format!("Page {}", glyph_set.pages.len() + 1),
                        ));
                        ui.close();
                    }
                    if ui.button("Rename…").clicked() {
                        actions.push(BrowserAction::RenameGlyphSet(glyph_set.id));
                        ui.close();
                    }
                    if ui.button("Duplicate").clicked() {
                        actions.push(BrowserAction::DuplicateGlyphSet(
                            glyph_set.id,
                            format!("{} copy", glyph_set.name),
                        ));
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Delete").clicked() {
                        actions.push(BrowserAction::RemoveGlyphSet(glyph_set.id));
                        ui.close();
                    }
                });
            }
        }
    });

    section(ui, "Export Configurations", "export_configs", |ui| {
        if model.export_configs.is_empty() {
            ui.weak("(none)");
        }
        for config in &model.export_configs {
            let selected = document.active && selected_export == Some(config.id);
            if ui.selectable_label(selected, &config.name).clicked() {
                *clicked_export = Some((document.id, config.id));
            }
        }
    });
}

/// The right-click menu for a page (spec/12 §12.2): rename, duplicate, move to another
/// glyph set of the **same geometry**, or delete.
fn page_context_menu(
    ui: &mut egui::Ui,
    model: &BrowserModel,
    glyph_set: &GlyphSetNode,
    page: &PageNode,
    actions: &mut Vec<BrowserAction>,
) {
    if ui.button("Rename…").clicked() {
        actions.push(BrowserAction::RenamePage(glyph_set.id, page.id));
        ui.close();
    }
    if ui.button("Duplicate").clicked() {
        actions.push(BrowserAction::DuplicatePage(
            glyph_set.id,
            page.id,
            format!("{} copy", page.name),
        ));
        ui.close();
    }
    ui.menu_button("Move to glyph set", |ui| {
        let targets: Vec<&GlyphSetNode> = model
            .glyph_sets
            .iter()
            .filter(|target| target.id != glyph_set.id && target.glyph_size == glyph_set.glyph_size)
            .collect();
        if targets.is_empty() {
            ui.weak("(no other same-size glyph set)");
        }
        for target in targets {
            if ui.button(&target.name).clicked() {
                actions.push(BrowserAction::MovePage {
                    from: glyph_set.id,
                    page: page.id,
                    to: target.id,
                });
                ui.close();
            }
        }
    });
    ui.separator();
    if ui.button("Delete").clicked() {
        actions.push(BrowserAction::RemovePage(glyph_set.id, page.id));
        ui.close();
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
    fn model_carries_export_configs_as_selectable_nodes() {
        let mut state = AppState::with_ids(Box::new(SequentialIdGen::new()));
        state.add_export_config("Text ROM".to_string());
        let model = browser_model(state.document());
        assert_eq!(model.export_configs.len(), 1);
        assert_eq!(model.export_configs[0].name, "Text ROM");
        assert_eq!(
            model.export_configs[0].id,
            state.selected_export_config().unwrap()
        );
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
