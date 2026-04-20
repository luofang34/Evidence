#!/usr/bin/env bash
# One-time install: point this repo's git hooks at `.githooks/`.
#
# Git's default hook path is `.git/hooks/`, which isn't version-
# controlled. Running this sets `core.hooksPath = .githooks` so the
# repo's committed hooks take effect. Idempotent — safe to re-run.
#
# Per-repo only (not `--global`); contributors who work on multiple
# repos keep their global `.git/hooks/` configuration untouched.

set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

chmod +x .githooks/*

git config core.hooksPath .githooks

printf 'installed: git hooks now run from .githooks/\n'
printf 'to disable: git config --unset core.hooksPath\n'
