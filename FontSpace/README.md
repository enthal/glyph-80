# FontSpace

FontSpace is a desktop application **and** a reusable Rust core for designing, organizing, transforming, comparing, rendering, and exporting monospaced raster fonts. It is the first sub-project of [Glyph-80](../README.md) — the tool that produces the glyph bitmaps the later hardware phases display.

- **Design docs:** [SPEC.md](SPEC.md) is the table of contents; each chapter under [spec/](spec/) is canonical for its area. Start with [spec/17-invariants-and-glossary.md](spec/17-invariants-and-glossary.md).
- **Milestones:** [PLAN.md](PLAN.md). **Milestone 1** (core model, JSON, operations, rendering, and the `fontspace` CLI) is complete; **Milestone 2** — the single-document editor GUI (`fontspace-egui`) — is now underway, starting with the tiled application shell.
- **Contributor/agent guide:** [CLAUDE.md](CLAUDE.md).

## Requirements

The Rust toolchain is pinned by [rust-toolchain.toml](rust-toolchain.toml) (channel `1.95.0`, with `rustfmt` and `clippy`). With `rustup` installed, the correct toolchain and components are fetched automatically the first time you run `cargo` in this directory — no manual `rustup` steps needed.

All commands below are run from this `FontSpace/` directory (it is its own Cargo workspace).

## Build & test

```sh
cargo build --workspace                                   # build
cargo test  --workspace                                   # run the full test suite
cargo clippy --workspace --all-targets -- -D warnings     # lint (warnings are errors)
cargo fmt --all                                           # format (--check to verify only)
```

Run `fmt`, `clippy`, and `test` before every commit. Treat clippy warnings as errors.

## Running

The **CLI** (`fontspace`, from `fontspace-cli`) is the runnable surface today — a thin adapter over the typed operations (spec/13):

```sh
cargo run -p fontspace-cli -- new demo.fontspace.json --name "Demo"
cargo run -p fontspace-cli -- info demo.fontspace.json
cargo run -p fontspace-cli -- render-text demo.fontspace.json \
  --glyph-set "Terminal 8x16" --glyphs 0x41-0x5A
cargo run -p fontspace-cli -- render-text demo.fontspace.json \
  --glyph-set "Terminal 8x16" --text-nl "Hello\nWorld"
cargo run -p fontspace-cli -- shift demo.fontspace.json \
  --glyph-set "Terminal 8x16" --pages Regular --glyphs A-Z --dx 1 --dy 0 --dry-run
```

`render-text` draws one **subject** — `--glyphs <selector>` (defaults to *all* glyphs when omitted), `--text <string>` (rendered as one line), or `--text-nl <string>` (newlines start a new line); the three are mutually exclusive. For `--text`/`--text-nl`, each input character maps to a `code` by its Unicode scalar (the byte value for Latin-1 input) and characters with no character-set entry are ignored.

Mutating commands (`set-pixels`, `shift`) write canonical JSON atomically and support `--dry-run`; `--seq` wires the deterministic id generator for reproducible fixtures. (`extract` and machine-readable JSON errors arrive with later milestones.)

The **GUI** (`fontspace-gui`, from `fontspace-egui`) is landing over Milestone 2. It opens the tiled application shell — the `egui_tiles` workspace with the default layout (documents · glyph editor · char-set/pages/preview tabs · inspector), plus **reset-to-default-layout** and **focus-glyph-editor** commands. The **glyph editor** is live and editable: a custom-painted view of the selected glyph's pixel matrix with square cells, grid lines, page guides, and a hover-coordinate readout. **Draw with the mouse** — the first pixel of a drag fixes the whole stroke to paint or erase, fast motion is interpolated so no cell is skipped, and the whole drag is **one undo entry** (`Cmd/Ctrl+Z` / `Cmd/Ctrl+Shift+Z`, or the Edit menu). The **page overview** tab shows every code in the page as a grid of thumbnails (blank where undrawn, dangling codes flagged); click one to edit it. The **character-set** tab is an ordered table (ordinal · code · character · label) — click a row to edit that code, and removing an entry first shows its **cascade impact** (how many glyphs it would delete) before you confirm. A collapsing **Guides** section in the editor adds/removes horizontal & vertical guides, toggles their visibility, and edits their position — each an undoable operation. The **Text Preview** tab renders editable sample text with the current page's glyphs (characters with no character-set entry are ignored). The **File** menu opens, saves (`Cmd/Ctrl+O` / `Cmd/Ctrl+S` / `Cmd/Ctrl+Shift+S`), and reverts `.fontspace.json` documents through native dialogs; **saves are atomic** (a crash never corrupts your file), the window title and menu bar show the document name with an unsaved-changes marker, and opening or reverting with unsaved edits asks first. It opens on a small in-memory starter document until you open a file. The window carries the app's embedded icon and its reverse-DNS desktop identity (`com.tjames.glyph80.FontSpace`); the Linux `.desktop` self-install and (in Milestone 4) native macOS menus follow in later slices.

```sh
cargo run -p fontspace-egui               # open the desktop shell
```

## Git pre-commit hook

The hook is **repo-level** (it lives at the Glyph-80 root) but installs itself through FontSpace's build, because FontSpace is the first sub-project to need it. It runs the cheap, deterministic checks — `fmt`, `clippy`, and the markdown no-hardwrap lint — but **not** the test suite (the tests-first workflow commits failing tests on purpose, so tests are gated in CI only).

**Install it once per clone by running the test suite:**

```sh
cargo test --workspace
```

That triggers [`cargo-husky`](https://github.com/rhysd/cargo-husky) (a dev-dependency of `fontspace-cli`) to copy the repo-root dispatcher at [`../.cargo-husky/hooks/pre-commit`](../.cargo-husky/hooks/pre-commit) into `.git/hooks/`. The dispatcher runs [`scripts/precommit.sh`](scripts/precommit.sh) only when `FontSpace/**` files are staged. A plain `cargo build` will **not** install it (it skips dev-dependencies).

Bypass the hook for a single commit with `git commit --no-verify` — only when intentional (e.g. committing a deliberately-failing test in the strict tests-first flow). Full design: [spec/19-ci-and-hooks.md](spec/19-ci-and-hooks.md) §19.2.

## Continuous integration

GitHub Actions runs [`.github/workflows/fontspace-ci.yml`](../.github/workflows/fontspace-ci.yml) on every PR (and on pushes to `main`), path-filtered to `FontSpace/**`. Jobs:

| Job | Runner(s) | Check |
| --- | --- | --- |
| `fmt` | ubuntu | `cargo fmt --all --check` |
| `clippy` | ubuntu + macOS | `cargo clippy --workspace --all-targets -- -D warnings` |
| `test` | ubuntu + macOS | `cargo test --workspace` |
| `markdown` | ubuntu | markdown no-hardwrap lint |

`clippy` and `test` run on a **macOS + Linux matrix** so the `cfg`-gated per-platform code (native macOS menus, Linux desktop integration) is actually compiled and checked on both. CI is the authoritative gate; the local hook is a fast convenience. Full design: [spec/19-ci-and-hooks.md](spec/19-ci-and-hooks.md).

GUI **snapshot tests** (`egui_kittest`) render through `wgpu` and are **pinned to Linux** (lavapipe) as the single canonical renderer — the macOS leg and local macOS dev skip them. Baselines are regenerated in an `ubuntu:24.04` + lavapipe container matching the runner (`UPDATE_SNAPSHOTS=1 cargo test`); review the changed `.png` before committing. See [spec/15-testing.md](spec/15-testing.md) §15.6.
