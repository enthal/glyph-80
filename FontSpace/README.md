# FontSpace

FontSpace is a desktop application **and** a reusable Rust core for designing, organizing, transforming, comparing, rendering, and exporting monospaced raster fonts. It is the first sub-project of [Glyph-80](../README.md) — the tool that produces the glyph bitmaps the later hardware phases display.

- **Design docs:** [SPEC.md](SPEC.md) is the table of contents; each chapter under [spec/](spec/) is canonical for its area. Start with [spec/17-invariants-and-glossary.md](spec/17-invariants-and-glossary.md).
- **Milestones:** [PLAN.md](PLAN.md). The repo is currently at **Milestone 0 — workspace bootstrap**: the crates exist and build, but the domain model, CLI, and GUI are not implemented yet.
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

There is nothing to run yet — Milestone 0 ships empty crates. The runnable surfaces arrive with later milestones and this section will grow to match:

- **CLI** (`fontspace-cli`) — Milestone 1. Headless `new` / `info` / `set-pixels` / `shift` / `render-text` / `extract` over `.fontspace.json` documents.
- **GUI** (`fontspace-egui`) — Milestone 2. The tiled desktop editor.

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
