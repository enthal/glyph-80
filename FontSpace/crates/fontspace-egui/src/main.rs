#![forbid(unsafe_code)]

//! Binary entry point: opens the FontSpace desktop window. The runnable shell is
//! deliberately trivial — it builds the native window (with the app's identity and
//! icon, spec/18 §18.1–18.2) and hands off to [`fontspace_egui::FontSpaceApp`]. The
//! Linux `.desktop` self-install and native macOS menus land in later slices (spec/18).

use fontspace_egui::{FontSpaceApp, platform};

fn main() -> eframe::Result {
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
