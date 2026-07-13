# 19. Continuous Integration and Local Hooks

Quality is enforced at two layers: a fast **local pre-commit hook** and an authoritative **CI gate**. They overlap deliberately — the cheap checks run in both, so a regression is caught at the keyboard *and* can never merge — but they are not identical, because the project's tests-first discipline forbids one obvious symmetry (§19.1).

## 19.1 What runs where, and why

| Check | Pre-commit hook | CI gate | Notes |
| --- | --- | --- | --- |
| `cargo fmt --all --check` | ✅ | ✅ | Formatting is deterministic and OS-independent; run once in CI. |
| `cargo clippy --workspace --all-targets -- -D warnings` | ✅ | ✅ (per-OS) | Run on **every** matrix OS in CI — clippy only lints the `cfg`-active code, and the platform code (chapter 18) is `cfg`-gated per OS. |
| markdown no-hardwrap lint | ✅ | ✅ | Deterministic lint; never committed red, so it is safe in the hook. |
| `cargo test --workspace` | ❌ | ✅ (per-OS) | **CI-only gate.** The strict layer ([15](15-testing.md)) requires committing a failing behavioural test *before* the code that greens it, so a hook that blocked failing tests would block the workflow. CI runs the full suite and gates merge — failures still block the world, just at the right boundary. |

`fmt` and `clippy` in both layers is the rule, not an accident: the hook gives instant feedback; CI is the thing branch protection trusts.

## 19.2 Local pre-commit hook

**Git hooks are repo-global** — there is exactly one `.git/hooks/pre-commit` for the whole repository, so there is no such thing as a subdirectory-scoped hook. The monorepo pattern is a single **repo-root dispatcher** that inspects the staged paths (`git diff --cached --name-only`) and runs each sub-project's checks only when that sub-project's files are staged. Each sub-project contributes a check *script*; FontSpace owns `FontSpace/scripts/precommit.sh` (fmt + clippy + markdown no-hardwrap), which the root dispatcher invokes when `FontSpace/**` is staged.

**Ownership and mechanism.** The hook *mechanism* — the dispatcher plus its installer — is a **repo-level** concern, not FontSpace's. It is set up once for the whole repo (FontSpace being the first sub-project to need it) and belongs to the repo root ([../../CLAUDE.md](../../CLAUDE.md)); FontSpace owns only `scripts/precommit.sh`. The repo uses **`cargo-husky`**: a `user-hooks` dev-dependency of `fontspace-cli` that, on the first `cargo test` inside `FontSpace/`, copies the committed repo-root dispatcher at `.cargo-husky/hooks/pre-commit` into `.git/hooks/`. There is no separate binary to install — the hook self-installs with the Rust toolchain a FontSpace developer already runs, and `cargo-husky` resolves `.cargo-husky/` at the git root, which is exactly where a repo-global hook belongs. The dispatcher is the path-filter: it runs `FontSpace/scripts/precommit.sh` only when `git diff --cached --name-only` shows staged `FontSpace/**` files. (Alternatives considered and rejected: **`lefthook`** and a hand-rolled `core.hooksPath` dispatcher both need a per-clone install step with an external binary or a `git config` line; the trade-off `cargo-husky` accepts is coupling the repo-global hook to a Rust crate build — to be revisited when a non-Rust sub-project lands, alongside the §19.5 sentinel note.)

The hook runs only the cheap checks (§19.1), never the test suite. It is opt-in per clone (installed by running `cargo test` once inside `FontSpace/`; git never auto-installs hooks, and neither does a plain `cargo build`, which skips dev-dependencies), so it is a convenience, never the source of truth — CI is. Bypass with `git commit --no-verify` only when the user explicitly asks (e.g. an intentional WIP commit); the repo-root command governance still applies.

## 19.3 CI gate

GitHub Actions, one workflow per sub-project (`.github/workflows/fontspace-ci.yml`), path-filtered so it runs only when FontSpace changes:

```yaml
on:
  pull_request:
    paths: ["FontSpace/**", ".github/workflows/fontspace-ci.yml"]
  push:
    branches: [main]
    paths: ["FontSpace/**", ".github/workflows/fontspace-ci.yml"]
```

Jobs (all `working-directory: FontSpace`, toolchain pinned by `rust-toolchain.toml`, with cargo/registry caching):

- **`fmt`** — `cargo fmt --all --check`. One runner (`ubuntu-latest`).
- **`clippy`** — `cargo clippy --workspace --all-targets -- -D warnings`. Matrix: `ubuntu-latest`, `macos-latest`.
- **`test`** — `cargo test --workspace`. Matrix: `ubuntu-latest`, `macos-latest`. Runs unit, round-trip, golden, property, integration, and `egui_kittest` snapshot tests. Snapshot tests render through `wgpu`; the Linux leg installs the software Vulkan driver (`mesa-vulkan-drivers` + `libvulkan1`, i.e. **lavapipe**) and runs with `WGPU_BACKEND=vulkan` so a headless runner can rasterize. Snapshots are **pinned to Linux** (chapter 15 §15.6) — the macOS leg compiles the GUI crate but its snapshot tests are `cfg`-excluded — so there is one canonical renderer and no per-OS baselines. A snapshot diff fails the job.
- **`markdown`** — the no-hardwrap lint over `FontSpace/**/*.md`. One runner.

The **OS matrix is required**, not cosmetic: without a macOS runner the `muda` menu path (chapter 18 §18.4) never compiles in CI; without Linux the desktop-integration path (§18.5) doesn't. Both are `cfg`-gated, so only the matching runner type-checks them.

Fuzz targets ([15](15-testing.md) §15.7) are **not** a PR gate — they run on a schedule or on demand, not on every PR.

## 19.4 Branch protection contexts

CI exists so `main`'s branch rule can require it — this closes the item deferred at repo setup (the rule couldn't require checks that didn't exist yet). Once this workflow runs, add these required status contexts to the `main` protection rule (job-name-qualified by the workflow):

```text
fontspace / fmt
fontspace / clippy (ubuntu-latest)
fontspace / clippy (macos-latest)
fontspace / test (ubuntu-latest)
fontspace / test (macos-latest)
fontspace / markdown
```

Keep `strict` (require branches up to date) on, matching the repo's other conventions.

## 19.5 The monorepo + required-checks gotcha

Path-filtering a required check has a well-known failure mode: if a PR touches **no** FontSpace files, the FontSpace workflow is skipped, and GitHub reports each required `fontspace / …` context as *pending forever* — blocking the merge. The fix is a tiny **status-sentinel** job that is *not* path-filtered and always runs, reporting success (or the aggregated result of the real jobs when they ran); the branch rule requires the sentinel, and the per-OS jobs are informational. Alternatively, GitHub's **merge queue** sidesteps it. This only bites once a **second** sub-project (or a root-only PR) exists — until then every PR touches `FontSpace/**` — so it is a Milestone-0 note to revisit when the next sub-project lands, not a v1 blocker.

## 19.6 Release CI

The release/packaging pipeline (signed macOS `.dmg`, Linux AppImage/`.deb`, version-free URLs) is separate from this PR gate and is specified as provisional in chapter 18 §18.8. It triggers on release tags, not on PRs, and builds only the changed sub-project.
