---
name: merge-review
description: Pre-merge code review of the current branch's diff for adherence to the spec and CLAUDE.md, plus correctness, DRYness, readability, and efficiency. Run as part of the merge ceremony (see repo-root CLAUDE.md) before squash-merging any PR, and any time a diff-level review is wanted. Not the same as /code-review — this one is spec-and-CLAUDE-aware and is the gate the merge ceremony requires.
---

# merge-review

A disciplined, spec-aware review of a branch's changes, run **before merge**. It exists because this repo's whole method is "the spec and CLAUDE.md are the source of truth" — a review that doesn't check the diff against them is missing the point. Use it as the review step of the merge ceremony (repo-root [CLAUDE.md](../../../CLAUDE.md) → Git protocol), or on demand for any branch.

## What it checks

Review the diff against these criteria, in priority order:

1. **Correctness** — logic bugs, off-by-one, integer overflow/underflow, boundary conditions, error handling, and whether invariants actually hold under every path. Scrutinize the tests: do they prove what they claim, or are they satisfiable by a trivial/no-op implementation? Name concrete failing inputs.
2. **Spec adherence** — compare against the canonical spec for the changed area (for FontSpace, `FontSpace/spec/*.md`; start from `17-invariants-and-glossary.md`). Flag any divergence in API signatures, type shapes, field names, semantics, or invariants. A normative change that did **not** update the spec in the same commit is a finding (the never-drift rule).
3. **CLAUDE.md adherence** — the nearest `CLAUDE.md` (sub-project file overrides root). For FontSpace: `#![forbid(unsafe_code)]`, no `unwrap()`/`expect()` in non-test code, no ambient nondeterminism (`Uuid::new_v4`/`now()`/unseeded RNG) in core, typed IDs, map-naming conventions, "make wrong states unrepresentable", structural safety, errors that identify object context, markdown never hard-wrapped, tests-first for the strict layer.
4. **DRYness** — duplication that should be factored; a formula or invariant expressed in more than one place.
5. **Readability & idiom** — naming, doc quality, idiomatic style for the language.
6. **Efficiency** — needless allocation or accidental O(n²), weighed against the spec's "correctness and clarity first" stance — don't invent micro-optimizations the spec disowns.

## How to run it

1. **Scope the diff.** Default to the current branch vs the merge base with `main`:
   `git merge-base HEAD origin/main` then `git diff <base>...HEAD --stat` and `--name-only`. If reviewing a GitHub PR, use its number.
2. **Spawn a review sub-agent** (read-only; it must not edit). Give it: the exact changed files, the specific spec chapters and `CLAUDE.md` to read **first**, and the six criteria above. Instruct it to cite `file:line`, rate each finding **High / Medium / Low**, distinguish real bugs from nits, briefly confirm what it verified as sound, and list spec/CLAUDE divergences explicitly. It may run `cargo clippy`/`cargo test` to confirm state but should focus on what tooling does **not** catch. For a large or multi-area diff, consider one sub-agent per area, in parallel.
3. **Triage and handle the findings yourself** (the sub-agent only reports):
   - Fix High and worthwhile Medium findings in the working tree.
   - For any normative change, update the spec in the **same commit** (never-drift).
   - Consciously **defer** anything not worth doing now — say so out loud, with the reason; don't silently drop it.
   - Re-run `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` until green.
4. **Report** a tight summary: the verdict, what was fixed, what was deferred and why.

## Definition of done

The review ran, its findings were either applied or explicitly deferred with a reason, and `fmt` + `clippy` + `test` are green. Only then does the merge ceremony proceed to the squash-merge.
