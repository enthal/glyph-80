# 18. Per-Platform Support

FontSpace targets **macOS and Linux** for v1 (Windows is post-v1, §18.7). It is an `eframe`/`egui`/`winit` app, so it inherits the same platform quirks Termica solved; this chapter adapts Termica's proven approach and records the hard-won details so we don't rediscover them. All platform integration lives in `fontspace-egui` — the only crate allowed to touch OS APIs (chapter 2). Its **pure** string/path builders (menu command tree, `.desktop` contents, exec-path resolution) are `cfg`-gated for their target but unit-tested on every platform, exactly as Termica does.

## 18.1 One desktop identity, one storage namespace

Two separate concerns, deliberately different strings:

- **`APP_ID` (reverse-DNS desktop identity)** — **`com.tjames.glyph80.FontSpace`** (the `glyph80` component leaves room for sibling Glyph-80 apps like a future terminal; the capitalized app component follows the `dev.warp.Warp` convention). This single identity is the Wayland/X11 `app_id`, the basename of the Linux `.desktop` and icon files, the `.desktop` `Icon=` value, `StartupWMClass`, **and** the packager `identifier`. If any of these diverge, the installed launcher and the running window become two different apps (generic icon, launcher won't merge with its window). It must equal whatever the packager config uses (§18.8). *Chosen pre-release; if a `glyph-80.com` domain is later acquired the id may migrate to a domain-based form — a low-cost change while no packages have shipped (there is no installed base to re-key), and it hardens only once §18.8 packaging ships.*
- **Storage namespace** — proposed short, stable `fontspace`. Names the on-disk directory holding real user data: `workspace.json`, preferences, recovery snapshots (chapter 11). Kept distinct from `APP_ID` so that changing the desktop identity never moves user data.

A unit test pins `APP_ID` to the packager identifier (Termica's `app_id_matches_packaged_identifier`), so the two can never silently drift.

## 18.2 App icon

The window/dock/taskbar icon is embedded in the binary (`include_bytes!("../assets/app_icon.png")`, relative to the crate's `src/`) so there is no runtime file dependency. It is decoded to `egui::IconData` for the viewport (via `eframe::icon_data::from_png_bytes`, so no extra image dependency), and written verbatim to the XDG icon path on Linux (§18.5). A decode failure is a cosmetic loss — degrade to no icon, never panic. **Asset:** `FontSpace/crates/fontspace-egui/assets/app_icon.png` (256×256 — the starter document's bitmap `A` on a dark rounded field; also the source for the `.icns`/packager icons).

## 18.3 Menus: one command registry, two presenters

FontSpace's menu bar (File · Edit · View · Character Set · Glyph · Page · Export · Window · Help — §12.12) is defined **once** as a data structure — a tree of menu commands, each with a stable id, label, optional accelerator, and an enabled/checked predicate. Two presenters consume that one tree:

- **macOS — native menu bar (`muda`).** The whole tree becomes a real `NSMenu` (§18.4). This is a deliberate expansion over Termica, which only put the app menu (About/Quit) natively; FontSpace renders every top-level menu natively, as macOS users expect.
- **Linux / Windows — in-window menu bar (`egui`).** The same tree is drawn in a top `egui` menu bar inside the window, because non-macOS desktops put the menu in the window.

Command activation is uniform: whichever presenter fires, it resolves to the same command id, which `update()` routes to the same handler. There is exactly one place each command is implemented. Enabled/checked state is computed from app state each frame (in-window) or refreshed on the native menu as state changes.

## 18.4 macOS specifics

Adopt Termica's `menu_macos` approach, generalized to the full menu tree.

- **Suppress winit's default menu.** In `eframe::NativeOptions`, set `event_loop_builder` to call `EventLoopBuilderExtMacOS::with_default_menu(false)`. winit's default Quit calls `[NSApplication terminate:]` directly, which would exit before `update()` can run confirm-on-unsaved-changes.
- **Install from the eframe creator callback, not earlier.** `muda`'s `init_for_nsapp()` requires `NSApplication` to already exist, which happens during winit's `resumed` event — i.e. *before* the creator runs, but after `NativeOptions` is built. The creator callback (`Box::new(|cc| …)`) is the correct install point.
- **Standard app menu, custom About/Quit.** Keep the conventional first submenu (About, Services, Hide / Hide Others / Show All, Quit). About and Quit are custom `MenuItem`s with ids (`"about"`, `"quit"`) so they surface as `MenuEvent`s we handle, instead of AppKit's default actions — this is what lets Quit route through our unsaved-changes guard and About open our own panel. Predefined items that bind to AppKit selectors (`hide:`, `hideOtherApplications:`, Services) are used as-is.
- **Consume events by polling.** In `update()`, drain `muda::MenuEvent::receiver().try_recv()` and match on the command id, setting the same app-state flags/commands the in-window presenter would (Termica: `"quit" => quit_requested = true`, `"about" => about_open = true`).
- **Lifetime: the menu must outlive the app; leak it.** `muda`'s `NSMenuItem` subclass stores a raw `*const MenuChild` ivar with **no retain count**. Dropping the `Menu`/handle invalidates those pointers and any custom-item click dereferences freed memory (`EXC_BAD_ACCESS`). Predefined selector-bound items would *appear* to work, masking the bug. Make the lifetime explicit with `Box::leak` — it is a one-time app singleton the OS owns for the process lifetime anyway. This caveat matters more for FontSpace than Termica because the full menu has many custom items.
- **Accelerators** are attached per item via `muda::accelerator` (`SUPER+…`). They must match the in-window shortcut table (§12.5) so both presenters agree.

## 18.5 Linux desktop integration

Linux desktops put an app's icon on its window — and merge a running window with its launcher — by matching the window's `app_id` to an installed `.desktop` entry of the same basename. FontSpace uses the single `APP_ID` for all of it and **self-installs** on every launch, so the icon/launcher work for AppImage and `cargo run` / dev builds where no package manager dropped an entry. Every step is best-effort: a failure never blocks startup, it just falls back to a generic icon.

Adopt Termica's `install_desktop_entry()` verbatim in spirit:

1. **Set the window `app_id`.** `ViewportBuilder::with_app_id(APP_ID)` drives the Wayland `app_id` / X11 `WM_CLASS`. `StartupWMClass` in the `.desktop` entry MUST equal it — that is what the compositor reports for the live window and keys the icon lookup on.
2. **Write the icon** to `$XDG_DATA_HOME/icons/hicolor/256x256/apps/<APP_ID>.png` (write-if-missing; the bytes are constant).
3. **Write the `.desktop` entry** to `$XDG_DATA_HOME/applications/<APP_ID>.desktop`, with `Icon=<APP_ID>`, `StartupWMClass=<APP_ID>`, and appropriate `Categories` (for FontSpace: `Graphics;Development;` — a font/bitmap tool). **"Steal on start":** rewrite only when the content differs, so the most-recently-launched build claims the entry (self-healing against a stale path, idempotent otherwise).
4. **`Exec` MUST be an absolute path that resolves.** GIO's `GDesktopAppInfo` loader — which gnome-shell's window tracker calls to map a window's `app_id` to an app (and icon) — runs `g_find_program_in_path` on `Exec` and returns NULL for the **entire entry** if it does not resolve. A bare `Exec=fontspace` is not on `PATH` for a dev build, so the window silently falls back to a generic icon even though every other field is right. Write `std::env::current_exe()`.
5. **AppImage exception.** Inside an AppImage, `current_exe()` is an ephemeral `/tmp/.mount_*` path that vanishes on exit — recording it breaks `Exec` next launch. Use the AppImage runtime's `$APPIMAGE` (the stable `.AppImage` file path) instead, but **only when genuinely inside that AppImage**: require `current_exe` to live under `$APPDIR`. That rejects a stray `$APPIMAGE` inherited from a parent process. Any mismatch falls back to `current_exe`. (Termica's `resolve_exec_path` is the exact logic; port it.)
6. **Nudge the shell.** After writing, best-effort `update-desktop-database <applications-dir>` (spawned, output nulled) so an already-running shell re-reads without a re-login. Absent tool is fine — the shell's file monitor still catches the write.
7. **One-time migration.** If an earlier basename was ever installed, remove the stale `.desktop` + icon so they don't shadow the new entry (Termica keeps a couple of `remove_file` best-effort calls for this).

Install runs **before** the window opens, so the compositor can match on first map.

### Cursor size & theme (accessibility)

winit decides the pointer's size/theme **solely** from `XCURSOR_SIZE` / `XCURSOR_THEME`; it does not read GSettings or the XDG portal the way GTK/Electron do. A user who enlarged their pointer would see a default-size cursor in FontSpace. Adopt Termica's `cursor_env` bridge: at startup, before the event loop, read the desktop's configured size/theme (GNOME `gsettings` today; the `org.freedesktop.portal.Settings` portal is the sandbox-safe follow-up) and, when the env vars are unset, **re-exec** with them populated. Re-exec (not `set_var`, which is `unsafe` under edition 2024 and we `forbid(unsafe_code)`) runs at most once, is a no-op when the vars are set or the desktop reports nothing, and never overrides an explicit `XCURSOR_*`. Gate the GSettings probe on a GNOME-family `XDG_CURRENT_DESKTOP` and fail closed everywhere else.

## 18.6 The pure/testable boundary

Keep the platform logic testable on any CI runner (Termica's pattern):

- Pure, `cfg`-independent functions: the menu command tree, `desktop_entry_contents(exec_path) -> String`, `desktop_exec_field(path) -> String` (both freedesktop escaping layers — quote `"` `` ` `` `$` `\`, and double any literal `%` so it isn't read as a `%f`/`%u` field code), and `resolve_exec_path(appimage, appdir, current_exe)`. Mark them `#[cfg_attr(not(target_os = "linux"), allow(dead_code))]` so non-Linux builds don't warn, and unit-test them on every platform.
- The filesystem-touching `install_desktop_entry()` and the `muda` install are `#[cfg(target_os = ...)]`-gated; their behavior is covered by the pure helpers plus manual/integration checks.

## 18.7 Windows (post-v1)

Windows is out of scope for v1 but nothing blocks it: the in-window menu presenter (§18.3) already works there, and desktop integration is the installer's job (no self-install dance). When it lands, add a `.msi`/`.exe` packaging target and a Windows icon; no core changes.

## 18.8 Distribution (provisional)

Packaging follows Termica's proven pipeline, adapted for the Glyph-80 monorepo; treated as forward-looking and mostly post-v1. Principles that are normative once shipped: version assigned at release time (never in feature PRs, per Conventional Commits — [../../CLAUDE.md](../../CLAUDE.md)); stable version-free download URLs; the same packaging locally and in CI; signing is additive (unsigned builds always work).

- **Formats** (via `cargo-packager`, one config → all formats): macOS `.app` in a `.dmg` (per-arch or a `lipo` universal); Linux **AppImage** (distro-agnostic, drives the self-install above) and `.deb`.
- **Monorepo wrinkle:** releases are per sub-project. Tag/asset names namespace the app (e.g. `fontspace-v0.1.0`, `FontSpace-macOS-AppleSilicon.dmg`), and CI builds only the changed sub-project. The repo has no release tooling yet; wiring it (release-plz or equivalent) is a Milestone-0/post-v1 task in [../PLAN.md](../PLAN.md).
- **macOS signing/notarization** is additive and identical in shape to Termica's (full-chain `.p12`, hardened runtime + timestamp, `notarytool` + `stapler`), gated on secrets being present. Linux needs no signing.
