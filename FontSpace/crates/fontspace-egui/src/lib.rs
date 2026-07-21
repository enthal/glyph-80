#![forbid(unsafe_code)]

//! FontSpace desktop GUI (spec/12).
//!
//! The top of the inward-dependency stack (spec/02): this crate may depend on every
//! core crate, but the core crates never depend on it. It owns the `egui` shell, the
//! `egui_tiles` workspace, input-gesture translation, and (later) clipboard/dialog
//! and platform integration. Font semantics, serialization, and transforms stay in
//! `fontspace-ops`/`-json`/`-render`; the GUI only constructs and invokes them.
//!
//! Non-trivial logic lives in pure, unit-tested functions (`layout`, and the
//! geometry/stroke math added by later slices), not in `egui` paint closures.

pub mod app;
pub mod charset_view;
pub mod document_browser;
pub mod editor;
pub mod export_view;
pub mod glyph_paint;
pub mod layout;
pub mod page_overview;
pub mod platform;
pub mod state;
pub mod text_preview;
pub mod workspace;

pub use app::FontSpaceApp;
pub use layout::{Pane, default_tree, panes_in};
pub use platform::{APP_ID, STORAGE_NAMESPACE};
pub use state::{AppState, GuardedIntent, Selection};
pub use workspace::{DocumentId, OpenDocument};
