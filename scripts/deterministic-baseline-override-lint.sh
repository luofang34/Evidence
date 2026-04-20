#!/usr/bin/env bash
# Cross-time-determinism lint (LLR-045). Compares two bundles'
# `deterministic_hash` values and, on mismatch, requires the PR
# body / commit message to contain a line matching
# `^Override-Deterministic-Baseline: .+` to accept the drift.
#
# Usage:
#
#   ./scripts/deterministic-baseline-override-lint.sh \
#       <prior_index_json> <current_index_json>
#
# Environment:
#
#   PR_BODY        — PR body text (CI injects via ${{ github.event.pull_request.body }}).
#                    Empty on push-to-main events.
#   COMMIT_MESSAGE — head commit message (push-mode fallback for the
#                    override line). Empty on pull_request events.
#   OVERRIDE_SKIP  — test hook; when set to "1", skip every external
#                    fetch step that would require network / gh auth.
#                    The caller supplies the two paths directly.
#
# Exit codes:
#
#   0 — hashes match, OR hashes differ but an override line is
#       present, OR prior bundle path doesn't exist (degraded
#       skip — the CI job's artifact-fetch step already logged
#       why).
#   1 — silent drift: hashes differ and no override line found.
#       Stderr carries both hashes + the expected override syntax.
#   2 — invocation / invariant error (missing argument, malformed
#       JSON, neither PR_BODY nor COMMIT_MESSAGE set but a
#       mismatch was found and the script needs to check one).
#
# Bash 3.2-portable (macOS system bash); no associative arrays,
# no `<<<`. Same idioms as `scripts/floors-lower-lint.sh`.

set -euo pipefail

prior="${1:-}"
current="${2:-}"

if [ -z "$prior" ] || [ -z "$current" ]; then
    printf 'usage: %s <prior_index_json> <current_index_json>\n' "$0" >&2
    exit 2
fi

if [ ! -f "$prior" ]; then
    printf 'cross-time-determinism: prior bundle index.json not found at %s; skipping.\n' "$prior" >&2
    printf '(This is the "prior-main artifact expired or does not exist" case — the live-compare gate is best-effort.)\n' >&2
    exit 0
fi

if [ ! -f "$current" ]; then
    printf 'cross-time-determinism: current bundle index.json not found at %s\n' "$current" >&2
    exit 2
fi

# Extract deterministic_hash from each index.json. `jq` is
# universally available on GitHub runners and in Nix shells; the
# script bails if it isn't, same pattern as other scripts here.
if ! command -v jq >/dev/null 2>&1; then
    printf 'cross-time-determinism: jq is required but not found on PATH\n' >&2
    exit 2
fi

prior_hash=$(jq -r '.deterministic_hash // empty' "$prior")
current_hash=$(jq -r '.deterministic_hash // empty' "$current")

if [ -z "$prior_hash" ]; then
    printf 'cross-time-determinism: prior bundle missing `deterministic_hash` field (%s); bundle shape changed?\n' "$prior" >&2
    exit 2
fi
if [ -z "$current_hash" ]; then
    printf 'cross-time-determinism: current bundle missing `deterministic_hash` field (%s)\n' "$current" >&2
    exit 2
fi

if [ "$prior_hash" = "$current_hash" ]; then
    # Match: every determinism-affecting input (Cargo.lock,
    # rust-toolchain.toml, rustflags, git state) is byte-identical
    # between the last successful main-branch build and this run.
    exit 0
fi

# Hashes differ — a determinism-affecting input changed. Accept
# only with an explicit override line in the PR body or commit
# message.
override_haystack=""
if [ -n "${PR_BODY:-}" ]; then
    override_haystack="${PR_BODY}"
fi
if [ -z "$override_haystack" ] && [ -n "${COMMIT_MESSAGE:-}" ]; then
    override_haystack="${COMMIT_MESSAGE}"
fi

if [ -n "$override_haystack" ]; then
    # Match `^Override-Deterministic-Baseline: .+` anywhere in
    # the haystack (line-anchored). Use a temp file for grep -E
    # so embedded newlines work across bash 3.2.
    tmp=$(mktemp)
    printf '%s\n' "$override_haystack" >"$tmp"
    if grep -qE '^Override-Deterministic-Baseline: .+' "$tmp"; then
        rm -f "$tmp"
        printf 'cross-time-determinism: hashes differ (prior=%s current=%s) but `Override-Deterministic-Baseline:` line is present; accepting.\n' \
            "$prior_hash" "$current_hash" >&2
        exit 0
    fi
    rm -f "$tmp"
fi

# Silent drift: fail loud.
cat >&2 <<EOMSG
cross-time-determinism: SILENT DRIFT DETECTED.

  prior main-branch deterministic_hash : ${prior_hash}
  current PR deterministic_hash        : ${current_hash}

The current PR changed a determinism-affecting input (Cargo.lock,
rust-toolchain.toml, RUSTFLAGS, or an env-var capture) relative to
the last successful main-branch build. If the change is
intentional, add this line to the PR body (or the head commit
message on push-to-main events):

  Override-Deterministic-Baseline: <one-sentence reason>

Examples:
  Override-Deterministic-Baseline: bumped serde_json to 1.0.130
  Override-Deterministic-Baseline: added -C opt-level=3 to RUSTFLAGS

If the change is unintentional, revert whichever input drifted.
Common culprits: forgot to commit a regenerated Cargo.lock after
a dep add, a local nightly rustc slipped past the pinned 1.95 in
rust-toolchain.toml, or a workflow-level env block mutated
RUSTFLAGS.
EOMSG

exit 1
