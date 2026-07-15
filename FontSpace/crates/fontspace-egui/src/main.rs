#![forbid(unsafe_code)]

//! Binary entry point: opens the FontSpace desktop window. It builds the native window
//! (with the app's identity and icon, spec/18 §18.1–18.2) and hands off to
//! [`fontspace_egui::FontSpaceApp`]. On Linux it also self-installs the desktop entry
//! and bridges the cursor size/theme before the window opens (spec/18 §18.5). Native
//! macOS menus land in Milestone 4.

use fontspace_egui::{FontSpaceApp, platform};

fn main() -> eframe::Result {
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
        Box::new(|cc| Ok(Box::new(FontSpaceApp::new(cc)))),
    )
}
