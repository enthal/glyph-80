#!/usr/bin/env bash
# FontSpace's pre-commit checks: the cheap, deterministic gate that runs at the
# keyboard. Invoked by the repo-root hook (installed via cargo-husky) only when
# FontSpace/** is staged. Deliberately does NOT run the test suite — the strict
# tests-first workflow commits failing tests on purpose, so the test gate is
# CI-only. See spec/19-ci-and-hooks.md.
set -euo pipefail

# Run from the FontSpace workspace root regardless of where git invoked us.
cd "$(dirname "$0")/.."

echo "[fontspace pre-commit] cargo fmt --all --check"
cargo fmt --all --check

echo "[fontspace pre-commit] cargo clippy --workspace --all-targets -- -D warnings"
cargo clippy --workspace --all-targets -- -D warnings

echo "[fontspace pre-commit] markdown no-hardwrap lint"
scripts/markdown-no-hardwrap.sh
