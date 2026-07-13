#![forbid(unsafe_code)]

//! Binary entry point: opens the FontSpace desktop window. The runnable shell is
//! deliberately trivial — it builds the native window and hands off to
//! [`fontspace_egui::FontSpaceApp`]. Platform integration (icon, native menus,
//! Linux desktop entry) lands in a later Milestone-2 slice (spec/18).

use fontspace_egui::FontSpaceApp;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("FontSpace")
            .with_inner_size([1200.0, 800.0]),
        ..Default::default()
    };
    eframe::run_native(
        "FontSpace",
        options,
        Box::new(|cc| Ok(Box::new(FontSpaceApp::new(cc)))),
    )
}
