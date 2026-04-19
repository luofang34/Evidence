#!/usr/bin/env bash
# Regenerate committed golden wire-shape fixtures.
#
# Run after an INTENTIONAL change to a fixture-locked wire shape
# (e.g. adding a field to `RuleEntry`). The byte-diff test in
# crates/cargo-evidence/tests/golden_fixtures.rs fires on any drift
# from these files, so this script is the documented way to roll
# forward — edit the RULES const, re-run this, commit both.
#
# Always run a full `cargo test` afterwards to make sure the
# regenerated fixture still parses under the test's shape checks.
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"
cargo build --release -p cargo-evidence --quiet
./target/release/cargo-evidence evidence rules --json \
    > crates/cargo-evidence/tests/fixtures/golden_rules.json
echo "regenerated: crates/cargo-evidence/tests/fixtures/golden_rules.json"
echo "diff (if any):"
git --no-pager diff -- crates/cargo-evidence/tests/fixtures/golden_rules.json | head -80 || true
