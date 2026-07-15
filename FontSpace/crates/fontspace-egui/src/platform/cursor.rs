//! Cursor size/theme bridge for Linux (spec/18 §18.5, accessibility).
//!
//! winit decides the pointer's size/theme **solely** from `XCURSOR_SIZE` /
//! `XCURSOR_THEME`; it does not read GSettings or the XDG portal the way GTK/Electron
//! do. A user who enlarged their pointer would otherwise see a default-size cursor. At
//! startup, before the event loop, we read the desktop's configured values (GNOME
//! `gsettings` today) and, when the env vars are unset, **re-exec** with them populated.
//! Re-exec (not `set_var`, which is `unsafe` under edition 2024 while we
//! `forbid(unsafe_code)`) runs at most once, is a no-op when the vars are already set or
//! the desktop reports nothing, and never overrides an explicit `XCURSOR_*`.
//!
//! The decision logic is pure and tested on every platform; only the probe + re-exec
//! touch the OS and are Linux-gated (§18.6).

/// Whether `XDG_CURRENT_DESKTOP` names a GNOME-family desktop (GNOME, `ubuntu:GNOME`,
/// `pop:GNOME`, Unity …). The GSettings probe is gated on this and fails closed
/// everywhere else (spec/18 §18.5).
pub fn is_gnome_family(xdg_current_desktop: Option<&str>) -> bool {
    xdg_current_desktop.is_some_and(|desktop| {
        desktop
            .split(':')
            .any(|part| part.eq_ignore_ascii_case("gnome") || part.eq_ignore_ascii_case("unity"))
    })
}

/// Parses a `gsettings get` value into a bare string: trims, strips a single layer of
/// surrounding single quotes (`'Yaru'` → `Yaru`), and treats empty as absent.
pub fn parse_gsettings_value(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    let unquoted = trimmed
        .strip_prefix('\'')
        .and_then(|inner| inner.strip_suffix('\''))
        .unwrap_or(trimmed);
    (!unquoted.is_empty()).then(|| unquoted.to_string())
}

/// The `XCURSOR_*` vars to add before re-exec, given the current env and the probed
/// desktop values (spec/18 §18.5): only vars that are currently unset **and** have a
/// non-empty probed value. An empty result means nothing to do — no re-exec — which is
/// also what breaks any re-exec loop (after re-exec the vars are set).
pub fn cursor_env_additions(
    current_size: Option<&str>,
    current_theme: Option<&str>,
    probed_size: Option<&str>,
    probed_theme: Option<&str>,
) -> Vec<(&'static str, String)> {
    let mut additions = Vec::new();
    if current_size.is_none()
        && let Some(size) = probed_size.filter(|value| !value.is_empty())
    {
        additions.push(("XCURSOR_SIZE", size.to_string()));
    }
    if current_theme.is_none()
        && let Some(theme) = probed_theme.filter(|value| !value.is_empty())
    {
        additions.push(("XCURSOR_THEME", theme.to_string()));
    }
    additions
}

/// Reads the desktop's configured cursor size/theme and, when `XCURSOR_*` are unset,
/// re-execs this process with them populated so winit picks them up (spec/18 §18.5).
/// Best-effort and at most once: a no-op off GNOME, when the vars are already set, or
/// when the desktop reports nothing; a failed re-exec simply continues unchanged.
#[cfg(target_os = "linux")]
pub fn bridge_cursor_env() {
    // Fail closed everywhere but GNOME-family desktops.
    if !is_gnome_family(std::env::var("XDG_CURRENT_DESKTOP").ok().as_deref()) {
        return;
    }
    let current_size = std::env::var("XCURSOR_SIZE").ok();
    let current_theme = std::env::var("XCURSOR_THEME").ok();
    if current_size.is_some() && current_theme.is_some() {
        return; // both already set — nothing to bridge
    }

    let probed_size = probe_gsettings("cursor-size");
    let probed_theme = probe_gsettings("cursor-theme");
    let additions = cursor_env_additions(
        current_size.as_deref(),
        current_theme.as_deref(),
        probed_size.as_deref(),
        probed_theme.as_deref(),
    );
    if additions.is_empty() {
        return;
    }

    // Re-exec with the vars added. `CommandExt::exec` is safe (returns only on failure)
    // and replaces the process image, so control never returns here on success.
    use std::os::unix::process::CommandExt as _;
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let mut command = std::process::Command::new(exe);
    command.args(std::env::args_os().skip(1));
    for (key, value) in additions {
        command.env(key, value);
    }
    let _ = command.exec(); // on failure, fall through and run normally
}

/// Reads one `org.gnome.desktop.interface` key via `gsettings`, or `None` on any error.
#[cfg(target_os = "linux")]
fn probe_gsettings(key: &str) -> Option<String> {
    let output = std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", key])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_gsettings_value(&String::from_utf8_lossy(&output.stdout))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gnome_family_detection() {
        assert!(is_gnome_family(Some("GNOME")));
        assert!(is_gnome_family(Some("ubuntu:GNOME")));
        assert!(is_gnome_family(Some("pop:GNOME")));
        assert!(is_gnome_family(Some("Unity")));
        assert!(!is_gnome_family(Some("KDE")));
        assert!(!is_gnome_family(Some("sway")));
        assert!(!is_gnome_family(None));
    }

    #[test]
    fn gsettings_value_parsing() {
        assert_eq!(parse_gsettings_value("'Yaru'"), Some("Yaru".to_string()));
        assert_eq!(parse_gsettings_value("  24\n"), Some("24".to_string()));
        assert_eq!(parse_gsettings_value("''"), None);
        assert_eq!(parse_gsettings_value("   "), None);
    }

    #[test]
    fn additions_only_for_unset_vars_with_probed_values() {
        // Both env vars set → nothing added (and thus no re-exec, no loop).
        assert!(
            cursor_env_additions(Some("32"), Some("Yaru"), Some("24"), Some("Adwaita")).is_empty()
        );
        // Size unset + probed → add size only; theme already set.
        assert_eq!(
            cursor_env_additions(None, Some("Yaru"), Some("24"), Some("Adwaita")),
            vec![("XCURSOR_SIZE", "24".to_string())]
        );
        // Both unset, both probed → add both.
        assert_eq!(
            cursor_env_additions(None, None, Some("24"), Some("Adwaita")),
            vec![
                ("XCURSOR_SIZE", "24".to_string()),
                ("XCURSOR_THEME", "Adwaita".to_string()),
            ]
        );
        // Unset but the desktop reported nothing (or empty) → nothing added.
        assert!(cursor_env_additions(None, None, None, Some("")).is_empty());
    }
}
