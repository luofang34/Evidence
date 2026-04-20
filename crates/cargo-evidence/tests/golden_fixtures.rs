//! Golden wire-shape fixtures. Byte-exact diff against committed
//! outputs so any accidental change to a MCP-facing surface fires
//! a targeted test.
//!
//! Today the fixture is the `rules --json` output — the most
//! important contract to lock because MCP consumes it
//! directly and a silent wire-shape change would break every
//! downstream agent. The fixture regenerator lives at
//! `tools/regen-golden-fixtures.sh` for intentional updates; running
//! it and committing is the documented way to roll forward.
//!
//! Bigger goldens (full `verify` or `check .` output) are deferred —
//! bundles embed commit-specific hashes, and a source-tree golden
//! requires freezing a synthetic workspace. coverage work
//! will provide a stable synthetic source for that second golden.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use assert_cmd::Command;

const GOLDEN_RULES: &[u8] = include_bytes!("fixtures/golden_rules.json");

fn cargo_evidence() -> Command {
    #[allow(deprecated)]
    Command::cargo_bin("cargo-evidence").unwrap()
}

/// Byte-diff `cargo evidence rules --json` against the committed
/// fixture. Any field-order change, added field, changed severity,
/// or renamed domain fires this with a readable diff. Regenerate
/// intentionally via `tools/regen-golden-fixtures.sh`.
#[test]
fn golden_rules_json_byte_diff() {
    let out = cargo_evidence()
        .args(["evidence", "rules", "--json"])
        .output()
        .expect("spawn");
    assert!(out.status.success(), "rules --json must exit 0");

    if out.stdout != GOLDEN_RULES {
        // Produce a useful error: show the first diverging line with
        // surrounding context. Pure byte compare would dump a 20KB
        // blob to the test output.
        let current = String::from_utf8_lossy(&out.stdout);
        let golden = String::from_utf8_lossy(GOLDEN_RULES);
        let mut diverge_line: Option<(usize, String, String)> = None;
        for (idx, (a, b)) in current.lines().zip(golden.lines()).enumerate() {
            if a != b {
                diverge_line = Some((idx + 1, a.to_string(), b.to_string()));
                break;
            }
        }
        match diverge_line {
            Some((lineno, current_line, golden_line)) => panic!(
                "rules --json diverged from golden at line {}:\n  current: {}\n  golden:  {}\n\n\
                 Regenerate with `tools/regen-golden-fixtures.sh` if the change is intentional.",
                lineno, current_line, golden_line
            ),
            None => panic!(
                "rules --json length diverged from golden (current {} bytes, golden {} bytes). \
                 Regenerate with `tools/regen-golden-fixtures.sh` if the change is intentional.",
                out.stdout.len(),
                GOLDEN_RULES.len()
            ),
        }
    }
}
