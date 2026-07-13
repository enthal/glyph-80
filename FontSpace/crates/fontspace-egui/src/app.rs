//! The application shell: an [`eframe::App`] hosting the `egui_tiles` workspace.
//!
//! This is deliberately thin (CLAUDE.md "the one architectural rule"): it owns
//! window chrome and tile layout, and will grow to construct and invoke
//! `fontspace-ops` operations. It never owns font semantics. All non-trivial logic
//! (the default layout) lives in pure, tested functions in [`crate::layout`].

use egui_tiles::{Tile, Tree};

use crate::editor::show_glyph_editor;
use crate::layout::{Pane, default_tree, panes_in};
use crate::state::AppState;

/// The FontSpace desktop application.
pub struct FontSpaceApp {
    tree: Tree<Pane>,
    state: AppState,
}

impl Default for FontSpaceApp {
    fn default() -> Self {
        Self {
            tree: default_tree(),
            state: AppState::default(),
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

    /// Builds the app around an explicit [`AppState`] — used by snapshot tests to
    /// wire a deterministic (sequential-id) document (spec/15 §15.6).
    pub fn with_state(state: AppState) -> Self {
        Self {
            tree: default_tree(),
            state,
        }
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
        self.handle_shortcuts(ui.ctx());

        egui::Panel::top("menu_bar").show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("Edit", |ui| {
                    if ui
                        .add_enabled(self.state.can_undo(), egui::Button::new("Undo"))
                        .clicked()
                    {
                        self.state.undo();
                        ui.close();
                    }
                    if ui
                        .add_enabled(self.state.can_redo(), egui::Button::new("Redo"))
                        .clicked()
                    {
                        self.state.redo();
                        ui.close();
                    }
                });
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

        // Split the borrow so the tiles behavior can hold `&mut state` while `tree` is
        // driven mutably (disjoint fields of `self`).
        let Self { tree, state } = self;
        let mut behavior = PaneBehavior { state };
        egui::CentralPanel::default().show(ui, |ui| {
            tree.ui(&mut behavior, ui);
        });
    }

    /// Consumes the undo/redo keyboard shortcuts (spec/12 §12.5). `COMMAND` maps to
    /// Cmd on macOS and Ctrl elsewhere, so both platforms match without extra code.
    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        use egui::{Key, KeyboardShortcut, Modifiers};
        let undo = KeyboardShortcut::new(Modifiers::COMMAND, Key::Z);
        let redo = KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
        // Consume redo FIRST: egui matches modifiers *logically*, so the plain-Cmd+Z
        // `undo` pattern also matches a Cmd+Shift+Z press. Claiming redo first removes
        // that event before `undo` can swallow it (redo's Cmd+Shift+Z pattern never
        // matches a bare Cmd+Z, since a required Shift can't be missing).
        let (do_redo, do_undo) =
            ctx.input_mut(|i| (i.consume_shortcut(&redo), i.consume_shortcut(&undo)));
        if do_redo {
            self.state.redo();
        } else if do_undo {
            self.state.undo();
        }
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

/// Renders panes against the shared [`AppState`]. The glyph editor is live (and
/// mutates state through pointer editing); the other views are placeholders until
/// their Milestone-2 slices land.
struct PaneBehavior<'a> {
    state: &'a mut AppState,
}

impl egui_tiles::Behavior<Pane> for PaneBehavior<'_> {
    fn tab_title_for_pane(&mut self, pane: &Pane) -> egui::WidgetText {
        pane.title().into()
    }

    fn pane_ui(
        &mut self,
        ui: &mut egui::Ui,
        _tile_id: egui_tiles::TileId,
        pane: &mut Pane,
    ) -> egui_tiles::UiResponse {
        match pane {
            Pane::GlyphEditor => show_glyph_editor(ui, self.state),
            other => placeholder(ui, *other),
        }
        egui_tiles::UiResponse::None
    }
}

/// A titled "coming soon" placeholder for views not yet implemented.
fn placeholder(ui: &mut egui::Ui, pane: Pane) {
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
        // Focusing raises the editor's tab; it never adds or drops panes. Compare as
        // sets: `make_active` mutates the tiles container, which reorders its
        // (hash-ordered) iteration — so the *set* of panes is what's invariant, not
        // the order.
        let before: std::collections::HashSet<_> = app.panes().into_iter().collect();
        app.focus_glyph_editor();
        let after: std::collections::HashSet<_> = app.panes().into_iter().collect();
        assert_eq!(before, after);
    }

    #[test]
    fn cmd_shift_z_redoes_rather_than_undoes() {
        // Regression: egui matches modifiers logically, so a naive undo-first consume
        // let Cmd+Shift+Z fall through to undo and made redo unreachable (spec §12.5).
        let mut app = FontSpaceApp::default();
        app.state.begin_stroke((0, 0));
        app.state.commit_stroke();
        app.state.undo();
        assert!(app.state.can_redo());

        let ctx = egui::Context::default();
        let raw = egui::RawInput {
            events: vec![egui::Event::Key {
                key: egui::Key::Z,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
            }],
            ..Default::default()
        };
        ctx.begin_pass(raw);
        app.handle_shortcuts(&ctx);
        let _ = ctx.end_pass();

        // Redo fired: the redo entry moved back onto the undo stack.
        assert!(!app.state.can_redo());
        assert!(app.state.can_undo());
    }
}
