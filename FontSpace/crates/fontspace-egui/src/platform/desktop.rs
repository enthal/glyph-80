//! Linux desktop-entry self-install (spec/18 §18.5–18.6).
//!
//! Linux desktops put an app's icon on its window — and merge a running window with
//! its launcher — by matching the window's `app_id` to an installed `.desktop` entry
//! of the same basename. FontSpace uses the single [`APP_ID`](super::APP_ID) for all of
//! it and **self-installs** on every launch, so the icon/launcher work for AppImage and
//! `cargo run` / dev builds where no package manager dropped an entry. Every step is
//! best-effort: a failure never blocks startup, it just falls back to a generic icon.
//!
//! The pure builders ([`desktop_entry_contents`], [`desktop_exec_field`],
//! [`resolve_exec_path`]) are `cfg`-independent and unit-tested on every platform; only
//! [`install_desktop_entry`] touches the filesystem and is Linux-gated (§18.6).

use std::path::{Path, PathBuf};

use super::APP_ID;

/// Escapes a path for a `.desktop` `Exec=` value (spec/18 §18.6). Two freedesktop
/// layers: the whole argument is wrapped in double quotes with `"` `` ` `` `$` `\`
/// backslash-escaped inside, and any literal `%` is doubled so it isn't parsed as a
/// `%f`/`%u` field code. Always quoting is valid and keeps the output uniform.
pub fn desktop_exec_field(path: &str) -> String {
    // Double literal `%` first (field-code layer). Its char set is disjoint from the
    // quote layer's (`"` `` ` `` `$` `\`), so the two passes don't interfere.
    let doubled = path.replace('%', "%%");
    let mut escaped = String::with_capacity(doubled.len() + 2);
    escaped.push('"');
    for ch in doubled.chars() {
        if matches!(ch, '"' | '`' | '$' | '\\') {
            escaped.push('\\');
        }
        escaped.push(ch);
    }
    escaped.push('"');
    escaped
}

/// The full `.desktop` entry contents for a resolved `exec_path` (spec/18 §18.5). The
/// `Icon=` and `StartupWMClass=` values are [`APP_ID`](super::APP_ID) so the compositor
/// maps the live window (whose `app_id` is also `APP_ID`) to this entry and its icon.
pub fn desktop_entry_contents(exec_path: &str) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Version=1.0\n\
         Name=FontSpace\n\
         Comment=Design and edit monospaced bitmap fonts\n\
         Exec={exec}\n\
         Icon={APP_ID}\n\
         Terminal=false\n\
         Categories=Graphics;Development;\n\
         StartupWMClass={APP_ID}\n\
         StartupNotify=true\n",
        exec = desktop_exec_field(exec_path),
    )
}

/// Resolves the `Exec` path (spec/18 §18.5 step 5). Inside an AppImage, `current_exe`
/// is an ephemeral `/tmp/.mount_*` path that vanishes on exit, so record the stable
/// `$APPIMAGE` path instead — but **only when genuinely inside that AppImage**, i.e.
/// `current_exe` lives under `$APPDIR`. That rejects a stray `$APPIMAGE` inherited from
/// a parent process; any mismatch falls back to `current_exe`.
pub fn resolve_exec_path(
    appimage: Option<&str>,
    appdir: Option<&str>,
    current_exe: &Path,
) -> PathBuf {
    if let (Some(appimage), Some(appdir)) = (appimage, appdir)
        && !appimage.is_empty()
        && !appdir.is_empty()
        && current_exe.starts_with(appdir)
    {
        return PathBuf::from(appimage);
    }
    current_exe.to_path_buf()
}

/// Self-installs the icon and `.desktop` entry so the window carries the app icon and
/// merges with its launcher (spec/18 §18.5). Best-effort throughout: any failure is
/// swallowed and startup continues with a generic icon. Runs before the window opens.
#[cfg(target_os = "linux")]
pub fn install_desktop_entry() {
    let Some(data_home) = xdg_data_home() else {
        return;
    };
    // A valid, resolvable absolute `Exec` is the whole point (§18.5 step 4); if we
    // can't learn our own path, an entry would be worse than none.
    let Ok(current_exe) = std::env::current_exe() else {
        return;
    };
    let exec = resolve_exec_path(
        std::env::var("APPIMAGE").ok().as_deref(),
        std::env::var("APPDIR").ok().as_deref(),
        &current_exe,
    );

    // 1. Icon: write-if-missing (the bytes are constant — §18.5 step 2).
    let icon_dir = data_home.join("icons/hicolor/256x256/apps");
    let icon_path = icon_dir.join(format!("{APP_ID}.png"));
    if !icon_path.exists() {
        let _ = std::fs::create_dir_all(&icon_dir);
        let _ = std::fs::write(&icon_path, super::APP_ICON_PNG);
    }

    // 2. `.desktop`: "steal on start" — rewrite only when the content differs, so the
    // most-recently-launched build claims the entry, idempotent otherwise (§18.5 step 3).
    let apps_dir = data_home.join("applications");
    let desktop_path = apps_dir.join(format!("{APP_ID}.desktop"));
    let contents = desktop_entry_contents(&exec.to_string_lossy());
    let differs = std::fs::read_to_string(&desktop_path)
        .map(|existing| existing != contents)
        .unwrap_or(true);
    if differs {
        let _ = std::fs::create_dir_all(&apps_dir);
        if std::fs::write(&desktop_path, &contents).is_ok() {
            // 3. Nudge an already-running shell to re-read (§18.5 step 6); absent tool
            // is fine — the shell's file monitor still catches the write.
            let _ = std::process::Command::new("update-desktop-database")
                .arg(&apps_dir)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();
        }
    }
    // Step 7 (one-time migration of a stale prior basename) is a no-op: this is the
    // first desktop identity FontSpace has shipped, so there is nothing to remove yet.
}

/// `$XDG_DATA_HOME`, or the `~/.local/share` default (spec/18 §18.5).
#[cfg(target_os = "linux")]
fn xdg_data_home() -> Option<PathBuf> {
    if let Some(explicit) = std::env::var_os("XDG_DATA_HOME")
        && !explicit.is_empty()
    {
        return Some(PathBuf::from(explicit));
    }
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exec_field_quotes_and_escapes() {
        assert_eq!(
            desktop_exec_field("/usr/bin/fontspace-gui"),
            "\"/usr/bin/fontspace-gui\""
        );
        // A literal `%` is doubled so it isn't read as a field code.
        assert_eq!(desktop_exec_field("/opt/a%b/x"), "\"/opt/a%%b/x\"");
        // The quote-layer chars are backslash-escaped inside the quotes.
        assert_eq!(desktop_exec_field(r#"/x/$y"#), "\"/x/\\$y\"");
        assert_eq!(desktop_exec_field(r#"/a"b"#), "\"/a\\\"b\"");
        assert_eq!(desktop_exec_field(r"/a\b"), "\"/a\\\\b\"");
    }

    #[test]
    fn entry_contents_bind_icon_and_wmclass_to_app_id() {
        let entry = desktop_entry_contents("/usr/bin/fontspace-gui");
        assert!(entry.starts_with("[Desktop Entry]\n"));
        assert!(entry.contains("Type=Application\n"));
        assert!(entry.contains(&format!("Icon={APP_ID}\n")));
        assert!(entry.contains(&format!("StartupWMClass={APP_ID}\n")));
        assert!(entry.contains("Categories=Graphics;Development;\n"));
        assert!(entry.contains("Exec=\"/usr/bin/fontspace-gui\"\n"));
    }

    #[test]
    fn resolve_exec_prefers_appimage_only_when_genuinely_inside_it() {
        let exe = PathBuf::from("/tmp/.mount_abc/usr/bin/fontspace-gui");
        // Genuinely inside the AppImage: current_exe is under $APPDIR → use $APPIMAGE.
        assert_eq!(
            resolve_exec_path(
                Some("/home/u/FontSpace.AppImage"),
                Some("/tmp/.mount_abc"),
                &exe
            ),
            PathBuf::from("/home/u/FontSpace.AppImage")
        );
        // $APPIMAGE set but current_exe is NOT under $APPDIR (stray inherited var) →
        // fall back to current_exe.
        assert_eq!(
            resolve_exec_path(
                Some("/home/u/Other.AppImage"),
                Some("/somewhere/else"),
                &exe
            ),
            exe
        );
        // No AppImage context → current_exe.
        assert_eq!(resolve_exec_path(None, None, &exe), exe);
        // Empty vars are treated as unset.
        assert_eq!(resolve_exec_path(Some(""), Some(""), &exe), exe);
    }
}
