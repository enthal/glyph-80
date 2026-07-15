//! Platform identity and OS integration (spec/18). `fontspace-egui` is the only crate
//! allowed to touch OS-facing concerns; this module holds the pieces that are shared
//! across platforms (the identity strings, the embedded icon) plus, in later slices,
//! the `cfg`-gated Linux desktop-entry install and macOS menu presenter.

use std::sync::Arc;

/// The one reverse-DNS **desktop identity** (spec/18 §18.1): the Wayland/X11
/// `app_id`, the basename of the Linux `.desktop` and icon files, the `.desktop`
/// `Icon=` value, `StartupWMClass`, and — once packaging lands (§18.8) — the packager
/// `identifier`. These must all be this exact string or the installed launcher and the
/// running window read as two different apps (generic icon, no window/launcher merge).
pub const APP_ID: &str = "com.tjames.glyph80.FontSpace";

/// The on-disk **storage namespace** (spec/18 §18.1): names the directory holding real
/// user data (`workspace.json`, preferences, recovery snapshots — chapter 11). Kept
/// deliberately distinct from [`APP_ID`] so changing the desktop identity never moves
/// user data.
pub const STORAGE_NAMESPACE: &str = "fontspace";

/// The window/dock/taskbar icon, embedded so there is no runtime file dependency
/// (spec/18 §18.2). Also the source bytes written verbatim to the XDG icon path on
/// Linux (§18.5) and the origin of the packager `.icns`/icons.
pub const APP_ICON_PNG: &[u8] = include_bytes!("../assets/app_icon.png");

/// Decodes [`APP_ICON_PNG`] into an [`egui::IconData`] for the viewport. A decode
/// failure is a cosmetic loss — the caller degrades to no icon, never panics
/// (spec/18 §18.2). Uses eframe's decoder so no extra image dependency is needed.
pub fn app_icon() -> Option<egui::IconData> {
    eframe::icon_data::from_png_bytes(APP_ICON_PNG).ok()
}

/// Builds the viewport with the app's title, size, desktop identity, and icon
/// (spec/18 §18.1–18.2, §18.5 step 1). `with_app_id` drives the Wayland `app_id` /
/// X11 `WM_CLASS`, which the Linux `.desktop` `StartupWMClass` must match (§18.5).
pub fn viewport() -> egui::ViewportBuilder {
    let mut builder = egui::ViewportBuilder::default()
        .with_title("FontSpace")
        .with_app_id(APP_ID)
        .with_inner_size([1200.0, 800.0]);
    if let Some(icon) = app_icon() {
        builder = builder.with_icon(Arc::new(icon));
    }
    builder
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_id_matches_packaged_identifier() {
        // Pins the desktop identity so it can never silently drift (spec/18 §18.1).
        // When the §18.8 packager config lands this should compare against its
        // `identifier` field; until then it guards the literal against accidental edits.
        assert_eq!(APP_ID, "com.tjames.glyph80.FontSpace");
        // Reverse-DNS shape: at least three dot-separated, non-empty components.
        assert!(
            APP_ID.split('.').filter(|part| !part.is_empty()).count() >= 3,
            "APP_ID must be reverse-DNS (a.b.c)"
        );
        assert!(!APP_ID.contains(char::is_whitespace));
    }

    #[test]
    fn storage_namespace_is_distinct_from_app_id() {
        // The whole point of the split (§18.1): they are different strings, so changing
        // the desktop identity never moves user data.
        assert_ne!(STORAGE_NAMESPACE, APP_ID);
        assert_eq!(STORAGE_NAMESPACE, "fontspace");
    }

    #[test]
    fn embedded_icon_decodes_to_square_rgba() {
        // The icon must decode (it ships in-tree); degrade-to-none is only for a
        // corrupt build, so here we assert the real asset is well-formed.
        let icon = app_icon().expect("embedded icon decodes");
        assert_eq!(icon.width, icon.height, "icon is square");
        assert_eq!(icon.width, 256, "icon is 256x256 (spec/18 §18.2)");
        assert_eq!(icon.rgba.len(), (icon.width * icon.height * 4) as usize);
    }
}
