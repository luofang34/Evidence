#!/usr/bin/env bash
# Cross-time-determinism lint (LLR-045). Compares the toolchain-
# sensitive fields of two `deterministic-manifest.json` bundles
# and, on mismatch, requires the PR body / commit message to
# contain a line matching `^Override-Deterministic-Baseline: .+`.
#
# Why compare fields, not `deterministic_hash`? The hash includes
# `git_sha` / `git_branch` / `git_dirty`, which always differ
# between a PR branch head and a main-branch commit. Comparing
# those would fire on every PR, which defeats the gate. The
# drift we actually want to catch is "the PR changed a
# reproducibility input" — rustc/cargo versions, Cargo.lock, the
# toolchain pin, RUSTFLAGS. Those are the six fields projected
# here.
#
# Usage:
#
#   ./scripts/deterministic-baseline-override-lint.sh \
#       <prior_manifest_json> <current_manifest_json>
#
# Environment:
#
#   PR_BODY        — PR body text (CI injects from
#                    `${{ github.event.pull_request.body }}`).
#                    Empty on push-to-main events.
#   COMMIT_MESSAGE — head commit message (push-mode fallback for
#                    the override line). Empty on pull_request
#                    events.
#
# Exit codes:
#
#   0 — fields match, OR fields differ but an override line is
#       present, OR prior manifest path doesn't exist (degraded
#       skip — the CI job's artifact-fetch step already logged
#       why).
#   1 — silent drift: fields differ and no override line found.
#       Stderr carries the unified diff + the expected override
#       syntax.
#   2 — invocation / invariant error (missing argument, `jq`
#       unavailable, malformed JSON).
#
# Bash 3.2-portable (macOS system bash); no associative arrays,
# no `<<<`. Same idioms as `scripts/floors-lower-lint.sh`.

set -euo pipefail

prior="${1:-}"
current="${2:-}"

if [ -z "$prior" ] || [ -z "$current" ]; then
    printf 'usage: %s <prior_manifest_json> <current_manifest_json>\n' "$0" >&2
    exit 2
fi

if [ ! -f "$prior" ]; then
    printf 'cross-time-determinism: prior manifest not found at %s; skipping.\n' "$prior" >&2
    printf '(This is the "prior-main artifact expired or does not exist" case — the live-compare gate is best-effort.)\n' >&2
    exit 0
fi

if [ ! -f "$current" ]; then
    printf 'cross-time-determinism: current manifest not found at %s\n' "$current" >&2
    exit 2
fi

if ! command -v jq >/dev/null 2>&1; then
    printf 'cross-time-determinism: jq is required but not found on PATH\n' >&2
    exit 2
fi

# The six toolchain-sensitive fields. Anything outside this
# projection either (a) always differs per commit (git_*) or (b)
# is ambient / structural (schema_version, profile) — neither
# should gate cross-commit drift.
project='{
    rustc: .rustc,
    cargo: .cargo,
    llvm_version: .llvm_version,
    cargo_lock_hash: .cargo_lock_hash,
    rust_toolchain_toml: .rust_toolchain_toml,
    rustflags: .rustflags
}'

# Produce canonicalized (sorted-key) JSON for each side so textual
# equality == structural equality.
prior_tf=$(jq -S "$project" "$prior" 2>/dev/null) || {
    printf 'cross-time-determinism: could not project prior manifest %s (malformed JSON?)\n' "$prior" >&2
    exit 2
}
current_tf=$(jq -S "$project" "$current" 2>/dev/null) || {
    printf 'cross-time-determinism: could not project current manifest %s (malformed JSON?)\n' "$current" >&2
    exit 2
}

if [ "$prior_tf" = "$current_tf" ]; then
    # Every reproducibility-affecting input is byte-identical
    # between the last successful main-branch build and this run.
    exit 0
fi

# Fields differ — accept only with an explicit override line.
# Concatenate both haystacks unconditionally: a PR body may be
# non-empty prose ("fix: bump serde") while the override line
# lives in the head commit message (squash-merge convention, or
# a PR body written without the override convention in mind).
# Matching either source is the intended contract.
override_haystack="${PR_BODY:-}
${COMMIT_MESSAGE:-}"

if [ -n "$(printf '%s' "$override_haystack" | tr -d '[:space:]')" ]; then
    tmp=$(mktemp)
    trap 'rm -f "$tmp"' EXIT
    printf '%s\n' "$override_haystack" >"$tmp"
    if grep -qE '^Override-Deterministic-Baseline: .+' "$tmp"; then
        printf 'cross-time-determinism: toolchain fingerprint differs vs prior main but `Override-Deterministic-Baseline:` line is present; accepting.\n' >&2
        # Log the diff anyway so the PR reviewer can see what
        # changed without hunting through two JSON files.
        printf '\n--- prior main toolchain projection\n%s\n' "$prior_tf" >&2
        printf '+++ current toolchain projection\n%s\n' "$current_tf" >&2
        exit 0
    fi
fi

# Silent drift: fail loud with a unified diff.
cat >&2 <<EOMSG
cross-time-determinism: SILENT DRIFT DETECTED.

The current PR changed a reproducibility-affecting input
(Cargo.lock, rust-toolchain.toml, RUSTFLAGS, or the installed
rustc/cargo versions) relative to the last successful main-branch
build. Projection diff:

--- prior main
${prior_tf}
+++ current PR
${current_tf}

If the change is intentional, add this line to the PR body (or
the head commit message on push-to-main events):

  Override-Deterministic-Baseline: <one-sentence reason>

Examples:
  Override-Deterministic-Baseline: bumped serde_json to 1.0.130
  Override-Deterministic-Baseline: added -C opt-level=3 to RUSTFLAGS

If the change is unintentional, revert whichever input drifted.
Common culprits: forgot to commit a regenerated Cargo.lock after
a dep add, a local nightly rustc slipped past the pinned
rust-toolchain.toml, or a workflow-level env block mutated
RUSTFLAGS.
EOMSG

exit 1
