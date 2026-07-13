//! The application shell: an [`eframe::App`] hosting the `egui_tiles` workspace.
//!
//! This is deliberately thin (CLAUDE.md "the one architectural rule"): it owns
//! window chrome and tile layout, and will grow to construct and invoke
//! `fontspace-ops` operations. It never owns font semantics. All non-trivial logic
//! (the default layout) lives in pure, tested functions in [`crate::layout`].

use egui_tiles::{Tile, Tree};

use crate::layout::{Pane, default_tree, panes_in};

/// The FontSpace desktop application.
pub struct FontSpaceApp {
    tree: Tree<Pane>,
    behavior: TreeBehavior,
}

impl Default for FontSpaceApp {
    fn default() -> Self {
        Self {
            tree: default_tree(),
            behavior: TreeBehavior,
        }
    }
}

impl FontSpaceApp {
    /// Constructs the app from the eframe creator context. The context is unused in
    /// this shell slice but is the install point for platform integration (menus,
    /// persistence) in later Milestone-2 slices (spec/18).
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Self::default()
    }

    /// The panes currently laid out. Exposed for tests of shell-level commands.
    pub fn panes(&self) -> Vec<Pane> {
        panes_in(&self.tree)
    }

    /// "Reset to default layout" command (spec/12 §12.1).
    pub fn reset_layout(&mut self) {
        self.tree = default_tree();
    }

    /// "Focus glyph editor" command (spec/12 §12.1): raise the tab holding the
    /// glyph editor so it becomes the active/visible pane in its tab strip.
    pub fn focus_glyph_editor(&mut self) {
        focus_pane(&mut self.tree, Pane::GlyphEditor);
    }

    /// Renders the whole shell (menu bar + tiled workspace) into `ui`. Split out of
    /// the `eframe::App` impl so it can be driven without an [`eframe::Frame`] — the
    /// `egui_kittest` snapshot harness calls it directly (spec/15 §15.6).
    pub fn show(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("menu_bar").show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("View", |ui| {
                    if ui.button("Reset layout to default").clicked() {
                        self.reset_layout();
                        ui.close();
                    }
                    if ui.button("Focus glyph editor").clicked() {
                        self.focus_glyph_editor();
                        ui.close();
                    }
                });
            });
        });

        egui::CentralPanel::default().show(ui, |ui| {
            self.tree.ui(&mut self.behavior, ui);
        });
    }
}

impl eframe::App for FontSpaceApp {
    // eframe 0.35 hands the root `Ui` directly (was `update(&mut self, &Context, …)`
    // in earlier versions); panels dock inside it.
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.show(ui);
    }
}

/// Raises the tab containing `target` so it is the active pane in its tab strip.
fn focus_pane(tree: &mut Tree<Pane>, target: Pane) {
    tree.make_active(|_id, tile| matches!(tile, Tile::Pane(pane) if *pane == target));
}

/// Renders panes. Every pane is a placeholder in this shell slice; the real widgets
/// (glyph editor, page overview, …) land in the following Milestone-2 slices.
struct TreeBehavior;

impl egui_tiles::Behavior<Pane> for TreeBehavior {
    fn tab_title_for_pane(&mut self, pane: &Pane) -> egui::WidgetText {
        pane.title().into()
    }

    fn pane_ui(
        &mut self,
        ui: &mut egui::Ui,
        _tile_id: egui_tiles::TileId,
        pane: &mut Pane,
    ) -> egui_tiles::UiResponse {
        ui.vertical(|ui| {
            ui.add_space(8.0);
            ui.heading(pane.title());
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new("(view arrives in a later Milestone-2 slice)")
                    .weak()
                    .italics(),
            );
        });
        egui_tiles::UiResponse::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reset_layout_restores_the_full_pane_set() {
        let mut app = FontSpaceApp::default();
        // Remove a *known* pane (not `tiles.iter().next()`, whose hash order is
        // non-deterministic and often yields a container, not a pane) so the count
        // reliably drops; resetting must bring it back.
        let editor = app.tree.tiles.find_pane(&Pane::GlyphEditor).unwrap();
        app.tree.tiles.remove(editor);
        assert!(app.panes().len() < Pane::ALL.len());
        app.reset_layout();
        assert_eq!(app.panes().len(), Pane::ALL.len());
    }

    #[test]
    fn focus_glyph_editor_is_a_safe_noop_on_the_pane_set() {
        let mut app = FontSpaceApp::default();
        // Focusing raises the editor's tab; it never adds or drops panes. (In the
        // default layout the editor is the dominant top pane, not inside a tab
        // strip, so there is no visible tab to raise yet — this guards the command
        // against panicking and against mutating the layout.)
        let before = app.panes();
        app.focus_glyph_editor();
        assert_eq!(app.panes(), before);
    }
}
