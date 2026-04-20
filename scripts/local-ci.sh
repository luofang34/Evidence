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

# Self-dogfood the rigor audit. The release binary just built
# runs doctor on the current workspace; any error-severity
# finding aborts with DOCTOR_FAIL. This matches the CI step in
# the Check job — catching rigor drift before push beats catching
# it on the PR.
log "cargo evidence doctor (self-dogfood)"
./target/release/cargo-evidence evidence doctor --format=jsonl

printf '\n== local-ci.sh: all gates pass ==\n'
