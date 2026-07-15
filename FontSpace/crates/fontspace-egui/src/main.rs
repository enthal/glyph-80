#![forbid(unsafe_code)]

//! Binary entry point: opens the FontSpace desktop window. It builds the native window
//! (with the app's identity and icon, spec/18 §18.1–18.2) and hands off to
//! [`fontspace_egui::FontSpaceApp`]. On Linux it also self-installs the desktop entry
//! and bridges the cursor size/theme before the window opens (spec/18 §18.5). Native
//! macOS menus land in Milestone 4.
//!
//! An optional path argument (`fontspace-gui file.fontspace.json`) opens that document
//! for editing on launch; with no argument the app opens on its in-memory starter.

use std::path::PathBuf;

use fontspace_egui::{FontSpaceApp, platform};

fn main() -> eframe::Result {
    // The first positional argument, if any, is a document to open on launch. Take it
    // as an OsString so non-UTF-8 paths still open (env::args would panic on those).
    let initial_file = std::env::args_os().nth(1).map(PathBuf::from);

    // Linux startup, before any window/event-loop setup (spec/18 §18.5): the cursor
    // bridge may re-exec the process, so it must run first; the desktop entry is
    // installed before the window maps so the compositor can match its icon on sight.
    #[cfg(target_os = "linux")]
    {
        platform::cursor::bridge_cursor_env();
        platform::desktop::install_desktop_entry();
    }

    let options = eframe::NativeOptions {
        viewport: platform::viewport(),
        ..Default::default()
    };
    eframe::run_native(
        "FontSpace",
        options,
        Box::new(move |cc| {
            let mut app = FontSpaceApp::new(cc);
            if let Some(path) = initial_file {
                app.open_startup_file(path);
            }
            Ok(Box::new(app))
        }),
    )
}
