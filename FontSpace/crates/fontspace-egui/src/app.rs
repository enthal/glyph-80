//! The application shell: an [`eframe::App`] hosting the `egui_tiles` workspace.
//!
//! This is deliberately thin (CLAUDE.md "the one architectural rule"): it owns
//! window chrome and tile layout, and will grow to construct and invoke
//! `fontspace-ops` operations. It never owns font semantics. All non-trivial logic
//! (the default layout) lives in pure, tested functions in [`crate::layout`].

use std::path::{Path, PathBuf};

use egui_tiles::{Tile, Tree};

use crate::charset_view::show_character_set;
use crate::document_browser::show_document_browser;
use crate::editor::show_glyph_editor;
use crate::layout::{Pane, default_tree, panes_in};
use crate::page_overview::show_page_overview;
use crate::state::{AppState, GuardedIntent};
use crate::text_preview::show_text_preview;

/// The FontSpace desktop application.
pub struct FontSpaceApp {
    tree: Tree<Pane>,
    state: AppState,
    /// The last OS window title we pushed, so we only send a viewport command when it
    /// actually changes. Sending one every frame would request a repaint every frame
    /// and the UI would never settle (breaking the snapshot harness's fixed-point run).
    last_title: String,
}

impl Default for FontSpaceApp {
    fn default() -> Self {
        Self {
            tree: default_tree(),
            state: AppState::default(),
            last_title: String::new(),
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
            last_title: String::new(),
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

        // Reflect the document name and dirty state in the OS window title (spec/12
        // §12.12), but only when it changes — a per-frame viewport command would keep
        // requesting repaints and never let the UI settle.
        let title = window_title(&self.state.document_name(), self.state.is_dirty());
        if self.last_title != title {
            self.last_title = title.clone();
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Title(title));
        }

        egui::Panel::top("menu_bar").show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Open...").clicked() {
                        self.action_open();
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Save").clicked() {
                        self.action_save();
                        ui.close();
                    }
                    if ui.button("Save As...").clicked() {
                        self.action_save_as();
                        ui.close();
                    }
                    ui.separator();
                    if ui
                        .add_enabled(self.state.can_close(), egui::Button::new("Close"))
                        .clicked()
                    {
                        self.action_close();
                        ui.close();
                    }
                    if ui
                        .add_enabled(self.state.can_revert(), egui::Button::new("Revert"))
                        .clicked()
                    {
                        self.action_revert();
                        ui.close();
                    }
                });
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

                // The document name and unsaved marker, right-aligned in the bar.
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(title_label(
                        &self.state.document_name(),
                        self.state.is_dirty(),
                    ));
                });
            });
        });

        self.show_discard_modal(ui.ctx());

        // A one-line status strip for the last save/open result or error (spec/12
        // §12.12): only present when there is something to report.
        if let Some(status) = self.state.status() {
            egui::Panel::bottom("status_bar").show(ui, |ui| {
                ui.label(status);
            });
        }

        // Split the borrow so the tiles behavior can hold `&mut state` while `tree` is
        // driven mutably (disjoint fields of `self`).
        let Self { tree, state, .. } = self;
        let mut behavior = PaneBehavior { state };
        egui::CentralPanel::default().show(ui, |ui| {
            tree.ui(&mut behavior, ui);
        });
    }

    /// Shows the unsaved-changes confirmation when a document-replacing action is
    /// pending (spec/12 §12.12). The buttons only set local flags; the intent is
    /// carried out after the modal closure so `self` is free to mutate.
    fn show_discard_modal(&mut self, ctx: &egui::Context) {
        if self.state.pending_discard().is_none() {
            return;
        }
        let name = self.state.document_name();
        let mut cancel = false;
        let mut discard = false;
        let modal = egui::Modal::new(egui::Id::new("discard_confirm")).show(ctx, |ui| {
            ui.set_width(320.0);
            ui.heading("Unsaved changes");
            ui.add_space(4.0);
            ui.label(format!("Discard unsaved changes to {name}?"));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
                if ui.button("Discard").clicked() {
                    discard = true;
                }
            });
        });
        if cancel || modal.should_close() {
            self.state.cancel_discard();
        } else if discard && let Some(intent) = self.state.take_pending_discard() {
            self.perform_guarded(intent);
        }
    }

    /// Consumes the undo/redo keyboard shortcuts (spec/12 §12.5). `COMMAND` maps to
    /// Cmd on macOS and Ctrl elsewhere, so both platforms match without extra code.
    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        use egui::{Key, KeyboardShortcut, Modifiers};
        let undo = KeyboardShortcut::new(Modifiers::COMMAND, Key::Z);
        let redo = KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
        let save = KeyboardShortcut::new(Modifiers::COMMAND, Key::S);
        let save_as = KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::S);
        let open = KeyboardShortcut::new(Modifiers::COMMAND, Key::O);
        let close = KeyboardShortcut::new(Modifiers::COMMAND, Key::W);
        // Consume the Shift-modified shortcuts (redo, save-as) FIRST: egui matches
        // modifiers *logically*, so a bare-Cmd pattern (undo, save) also matches its
        // Cmd+Shift press. Claiming the Shift variant first removes that event before
        // the bare pattern can swallow it (the reverse can't misfire — a required
        // Shift can't be absent from a bare-Cmd press). Same fix as spec §12.5.
        let (do_redo, do_save_as, do_undo, do_save, do_open, do_close) = ctx.input_mut(|i| {
            (
                i.consume_shortcut(&redo),
                i.consume_shortcut(&save_as),
                i.consume_shortcut(&undo),
                i.consume_shortcut(&save),
                i.consume_shortcut(&open),
                i.consume_shortcut(&close),
            )
        });
        if do_redo {
            self.state.redo();
        } else if do_undo {
            self.state.undo();
        }
        if do_save_as {
            self.action_save_as();
        } else if do_save {
            self.action_save();
        }
        if do_open {
            self.action_open();
        }
        if do_close {
            self.action_close();
        }
    }

    // --- File actions (spec/12 §12.12). Each is the thin glue between a native file
    // dialog (`rfd`) and the pure state transitions on `AppState`; the atomic read/
    // write lives in `fontspace_json`. Dialogs run only on user action, never in
    // tests, so this layer stays free of headless concerns. ---

    /// Open: pick a file and open it as a new document. No unsaved-changes guard — the
    /// current document is kept (it moves to the background), nothing is discarded.
    fn action_open(&mut self) {
        self.open_via_dialog();
    }

    /// Save: write to the bound file, or fall back to Save As when never saved.
    fn action_save(&mut self) {
        match self.state.path().map(Path::to_path_buf) {
            Some(path) => self.write_to(path),
            None => self.action_save_as(),
        }
    }

    /// Save As: pick a destination, write there, and bind the document to it.
    fn action_save_as(&mut self) {
        let suggested = self
            .state
            .path()
            .and_then(|path| path.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "untitled.fontspace.json".to_string());
        let Some(path) = rfd::FileDialog::new()
            .add_filter("FontSpace document", &["json"])
            .set_file_name(suggested)
            .save_file()
        else {
            return; // dialog cancelled
        };
        self.write_to(path);
    }

    /// Revert: reload the bound file, discarding edits (guarded by confirmation).
    fn action_revert(&mut self) {
        if self.state.can_revert() && self.state.begin_guarded(GuardedIntent::Revert) {
            self.do_revert();
        }
    }

    /// Close: close the active document (guarded by confirmation when it is dirty).
    /// Proceeds immediately when clean; a no-op when it is the only open document.
    fn action_close(&mut self) {
        if self.state.can_close() && self.state.begin_guarded(GuardedIntent::Close) {
            self.state.close_active();
        }
    }

    /// Runs a confirmed discard action (the user chose "Discard" in the guard modal).
    fn perform_guarded(&mut self, intent: GuardedIntent) {
        match intent {
            GuardedIntent::Revert => self.do_revert(),
            GuardedIntent::Close => self.state.close_active(),
        }
    }

    fn open_via_dialog(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("FontSpace document", &["json"])
            .pick_file()
        else {
            return; // dialog cancelled
        };
        if let Some(outcome) = self.read_or_report(&path) {
            self.state.open_document(outcome, path);
        }
    }

    fn do_revert(&mut self) {
        let Some(path) = self.state.path().map(Path::to_path_buf) else {
            return;
        };
        if let Some(outcome) = self.read_or_report(&path) {
            self.state.load_document(outcome, path);
        }
    }

    /// Reads a document file, reporting any failure in the status strip and returning
    /// `None` (a failed load never disturbs the open documents — spec/16 §16.2).
    fn read_or_report(&mut self, path: &Path) -> Option<fontspace_json::LoadOutcome> {
        match fontspace_json::read_document(path) {
            Ok(outcome) => Some(outcome),
            Err(err) => {
                self.state.set_error(err.to_string());
                None
            }
        }
    }

    /// Writes the document to `path` atomically and binds it, or reports the failure.
    fn write_to(&mut self, path: PathBuf) {
        match fontspace_json::write_document(&path, self.state.document()) {
            Ok(()) => self.state.mark_saved(path),
            Err(err) => self.state.set_error(format!("Save failed: {err}")),
        }
    }
}

/// The in-window document label: the file name (or "Untitled") with a leading `*`
/// when there are unsaved changes. ASCII marker so it renders in every font.
fn title_label(name: &str, dirty: bool) -> String {
    if dirty {
        format!("* {name}")
    } else {
        name.to_string()
    }
}

/// The OS window title: the document label followed by the app name.
fn window_title(name: &str, dirty: bool) -> String {
    format!("{} - FontSpace", title_label(name, dirty))
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
            Pane::PageOverview => show_page_overview(ui, self.state),
            Pane::CharacterSet => show_character_set(ui, self.state),
            Pane::TextPreview => show_text_preview(ui, self.state),
            Pane::DocumentBrowser => show_document_browser(ui, self.state),
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

    #[test]
    fn title_label_and_window_title_mark_unsaved_edits() {
        assert_eq!(title_label("Untitled", false), "Untitled");
        assert_eq!(title_label("Untitled", true), "* Untitled");
        assert_eq!(
            title_label("demo.fontspace.json", true),
            "* demo.fontspace.json"
        );
        assert_eq!(
            window_title("demo.fontspace.json", false),
            "demo.fontspace.json - FontSpace"
        );
        assert_eq!(window_title("Untitled", true), "* Untitled - FontSpace");
    }

    #[test]
    fn cmd_w_closes_the_active_document() {
        // Cmd+W closes the active document when another is open, promoting the
        // background document (spec §12.5, §12.12). A clean document needs no guard.
        let mut app = FontSpaceApp::default();
        let first_id = app.state.open_documents().next().unwrap().0.id;
        let outcome = fontspace_json::LoadOutcome {
            document: app.state.document().clone(),
            warnings: Vec::new(),
        };
        app.state
            .open_document(outcome, std::path::PathBuf::from("/tmp/b.fontspace.json"));
        assert_eq!(app.state.open_document_count(), 2);

        let ctx = egui::Context::default();
        let raw = egui::RawInput {
            events: vec![egui::Event::Key {
                key: egui::Key::W,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::COMMAND,
            }],
            ..Default::default()
        };
        ctx.begin_pass(raw);
        app.handle_shortcuts(&ctx);
        let _ = ctx.end_pass();

        // The second (active) document closed; the first is active again.
        assert_eq!(app.state.open_document_count(), 1);
        assert_eq!(app.state.open_documents().next().unwrap().0.id, first_id);
    }

    #[test]
    fn revert_on_a_dirty_document_arms_the_confirmation() {
        // Revert discards the active document's edits, so a dirty, file-bound document
        // must confirm before reloading — and the guard fires *before* any disk access,
        // so calling the action is safe (and testable) headlessly. (Open, by contrast,
        // opens a new document and is unguarded.)
        let mut app = FontSpaceApp::default();
        app.state
            .mark_saved(std::path::PathBuf::from("/tmp/demo.fontspace.json"));
        app.state.begin_stroke((0, 0));
        app.state.commit_stroke();
        assert!(app.state.can_revert());

        app.action_revert();
        assert_eq!(app.state.pending_discard(), Some(GuardedIntent::Revert));

        // Cancelling keeps the (still dirty) document.
        app.state.cancel_discard();
        assert_eq!(app.state.pending_discard(), None);
        assert!(app.state.is_dirty());
    }
}
