#!/usr/bin/env bash
# Mirror of the cargo commands run by `.github/workflows/ci.yml`.
#
# Single entry point for "run the full gate locally before pushing."
# Pre-push / pre-PR call this; the `local_ci_mirrors_workflow`
# integration test in `crates/evidence-core/tests/` asserts that every
# cargo command gated in the workflow also appears here. If CI adds a
# new cargo flag, the test fires on the PR that missed it.
#
# **Contract**: runs what CI runs, nothing less — but potentially
# slightly more. Specifically: CI's Doc gate is Linux-only (macOS /
# Windows runners skip it to save minutes); this script runs the doc
# gate on every host. Net effect: macOS / Windows contributors catch
# doc-link drift locally before the ubuntu runner catches it in CI.
# The extra strictness is a feature, not drift.
#
# The script is intentionally flat — no conditional skips, no
# "quick mode." A subset run is exactly the failure mode PR #49 hit
# (the `RUSTDOCFLAGS` doc gate was only partially run locally and
# CI caught what the subset missed). If you need a faster loop, run
# individual `cargo` commands directly; the contract of this script
# is "runs what CI runs, nothing less."

set -euo pipefail

# Pin `RUSTFLAGS` / `RUSTDOCFLAGS` to the exact values CI sets. These
# are env-level in the workflow (`env:` block near the top), applied
# to every cargo invocation in the check job.
export RUSTFLAGS="${RUSTFLAGS:--D warnings}"
export CARGO_TERM_COLOR="${CARGO_TERM_COLOR:-always}"

log() { printf '\n== %s ==\n' "$1"; }

log "cargo fmt --all --check"
cargo fmt --all -- --check

log "cargo clippy --workspace --all-targets -- -D warnings"
cargo clippy --workspace --all-targets -- -D warnings

log "cargo test --workspace"
cargo test --workspace

# Doc gate: the rustdoc-specific escalations catch `broken_intra_doc_links`
# and (future) HTML / code-block errors. `missing_docs` comes from
# `RUSTFLAGS=-D warnings` combined with the workspace-level
# `missing_docs = "warn"` lint. Keeping both env vars set here
# matches CI's "Doc gate" step byte-for-byte.
log "cargo doc --workspace --no-deps (with broken + private intra-doc link gates)"
RUSTDOCFLAGS="-D rustdoc::broken_intra_doc_links -D rustdoc::private_intra_doc_links -D warnings" \
    cargo doc --workspace --no-deps

log "cargo build --workspace --release"
cargo build --workspace --release

# Workflow static-analysis. Catches typo'd input names, unresolvable
# secret refs, bad `if:` expressions, malformed matrix — the kind of
# workflow bug that only surfaces when the trigger actually fires
# (manual dispatch, tag push). Local preview before push so workflow
# changes don't eat a CI cycle to learn "that secret name doesn't
# exist."
#
# Soft-skip if actionlint isn't on PATH: a contributor without
# actionlint installed can still run the script to exercise the cargo
# gates; CI's actionlint job catches the static-analysis gap. The
# `local_ci_mirrors_workflow` lock test enforces that the step stays
# listed here.
log "actionlint (workflow static analysis)"
if command -v actionlint >/dev/null 2>&1; then
    actionlint -shellcheck= .github/workflows/*.yml
else
    printf '  actionlint not found on PATH; skipping locally (CI covers)\n'
fi

# Self-dogfood the rigor audit. The release binary just built
# runs doctor on the current workspace; any error-severity
# finding aborts with DOCTOR_FAIL. This matches the CI step in
# the Check job — catching rigor drift before push beats catching
# it on the PR.
log "cargo evidence doctor (self-dogfood)"
./target/release/cargo-evidence evidence doctor --format=jsonl

# Smoke-test the MCP wrapper. One scripted init + tools/list +
# tools/call (evidence_rules) round-trip exercises the full
# rmcp stack (handshake, routing, Parameters<T> decode, Json<T>
# response). Asserts the count field matches the library const
# — same invariant as the integration test
# `evidence_rules_count_matches_library_const`, but from a
# shell so a broken release build surfaces immediately.
log "mcp-evidence smoke (stdio handshake + evidence_rules)"
EXPECTED_RULES_COUNT=$(./target/release/cargo-evidence evidence rules --json \
    | python3 -c 'import sys, json; print(len(json.load(sys.stdin)))')
MCP_COUNT=$(printf '%s\n' \
    '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"local-ci","version":"0"}}}' \
    '{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}' \
    '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"evidence_rules","arguments":{}}}' \
    | ./target/release/mcp-evidence 2>/dev/null \
    | python3 -c 'import sys, json
for line in sys.stdin:
    d = json.loads(line)
    sc = d.get("result", {}).get("structuredContent")
    if sc is not None:
        print(sc["count"])
        break')
if [ "$MCP_COUNT" != "$EXPECTED_RULES_COUNT" ]; then
    printf '  mcp-evidence returned count=%s but CLI rules --json reports %s\n' \
        "$MCP_COUNT" "$EXPECTED_RULES_COUNT" >&2
    exit 1
fi
printf '  mcp-evidence evidence_rules round-trip OK (count=%s)\n' "$MCP_COUNT"

printf '\n== local-ci.sh: all gates pass ==\n'
