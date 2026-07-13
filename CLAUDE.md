# Claude Code Instructions

## The project

Glyph-80 is a from-scratch text-display system spanning **hardware** (initially breadboard), **firmware** (microcode, ROMs, EEPROM images), and **software** (tools). It is built up in phases — see the roadmap in [README.md](README.md) — from an LED-matrix glyph cycler up to a microcode-driven VGA terminal with its own control-code language.

The repo is organized as **one directory per sub-project**. Each sub-project is self-contained: its own build, its own tests, its own docs, and its own `CLAUDE.md` with the detail specific to that layer. This root file holds only what is true across the whole repo.

## Sub-projects

Work happens inside a sub-project directory. **Before working in a sub-project, read its `CLAUDE.md`** — it is the source of truth for that layer and overrides anything general here.

| Directory | `CLAUDE.md` | What it is |
| --- | --- | --- |
| [FontSpace/](FontSpace/) | [FontSpace/CLAUDE.md](FontSpace/CLAUDE.md) | Bitmap-font creation tool, in Rust. First sub-project. |

_(Later hardware/firmware sub-projects will be added here as they begin.)_

When a sub-project directory has its own `CLAUDE.md`, that file is canonical for everything under it. If this root file and a sub-project file disagree, the sub-project file wins for work inside that directory — but flag the conflict to the user so we can reconcile.

## Working with the user

- This is an exploratory, phased project. Work in conversation with the user; don't race ahead to later phases before the current one is real.
- Design docs and specs are the source of truth. When code and a design doc disagree, **raise it with the user** and decide whether to change the code or the doc — never silently drift.
- When mentioning a GitHub Issue or PR, use its number as a clickable link (e.g. `[#42](https://github.com/enthal/glyph-80/issues/42)`).

## Markdown

- **Markdown is never hard-wrapped.** One logical line per paragraph, list item, and block-quote — let the editor/renderer soft-wrap. Do not insert newlines mid-paragraph to hit a column width; it carries no meaning and churns diffs. Applies to every `.md` file in the repo.

## Git protocol

- **Commit messages follow [Conventional Commits v1.0.0](https://www.conventionalcommits.org/en/v1.0.0/).** Every commit subject is `<type>[optional scope]: <description>`, where `type` is one of `feat`, `fix`, `build`, `chore`, `ci`, `docs`, `style`, `refactor`, `perf`, `test`. Use a scope (usually the sub-project) when it helps: `feat(fontspace): …`. A breaking change is marked with `!` after the type/scope (`feat(fontspace)!: …`) and/or a `BREAKING CHANGE:` footer. This is required, not a preference — it governs how history reads and enables tooling.
- **Never commit directly to `main`** (except the initial repo-setup commit). All changes land via a feature branch → PR → squash merge.
- **Branch naming:** `<kind>/<slug>` where `kind` is one of `feat`, `fix`, `refactor`, `spec`, `chore`, `test`. Dashes in the slug, not underscores. Examples: `feat/fontspace-editor`, `chore/repo-setup`.
- **Start the branch first**, from an up-to-date `main`: `git switch -c <kind>/<slug>`. If you catch yourself having already committed on local `main`: `git switch -c <kind>/<slug>` (takes the commits with you), then `git switch main && git reset --hard origin/main`.
- **One PR per logical change.** Small and reviewable. If a GitHub Issue exists, include `Closes #<n>` in the PR description.
- **Squash on merge**; keep the branch. After merging, switch to `main` and pull.
- Before merging, update [README.md](README.md) (and the relevant sub-project docs) to reflect any user-facing changes.
- **Merge ceremony.** Before squash-merging any PR, in order: (1) run the [`merge-review`](.claude/skills/merge-review/SKILL.md) skill over the branch diff and handle its findings — fix the worthwhile ones (spec-syncing in the same commit), or defer the rest out loud with a reason; (2) confirm `fmt` + `clippy` + `test` are green and CI has passed; (3) update `README.md`/sub-project docs for any user-facing change. Only then squash-merge, switch to `main`, and pull. The review is not optional — it is the gate that keeps code honest against the spec and CLAUDE.md.

## Command governance

- Use relative paths in shell commands, not absolute paths. Avoid `git -C <abs-path>`; it breaks project-level Claude permissions.
- Don't skip hooks (`--no-verify`) or bypass signing unless the user explicitly asks. If a hook fails, fix the underlying issue.
- **The git pre-commit hook mechanism is repo-level and lives at the root:** a dispatcher at `.cargo-husky/hooks/pre-commit` that each sub-project extends. It is installed into `.git/hooks/` by `cargo-husky` on the first `cargo test` in a Rust sub-project (currently FontSpace). See [FontSpace/spec/19-ci-and-hooks.md](FontSpace/spec/19-ci-and-hooks.md) §19.2.
- **Only kill processes you started.** Capture the PID of anything you launch and kill *only* that PID. Never `pkill`/`kill` by name or pattern.
- **Worktrees live under `./.claude/worktrees/<slug>`**, not in sibling directories.

## Watching CI on open PRs

- **Use the `Monitor` tool**, not polling loops, to watch `gh pr checks` / `gh pr list`. Keep working in parallel; let the harness notify you on state changes. Do not write `until` loops or `sleep N && gh pr checks` chains.
