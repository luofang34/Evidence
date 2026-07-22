//! Shared helpers for the `init` scaffold integration tests.
//!
//! Included into each test target via `#[path]` (the same pattern
//! `walker_helpers.rs` uses) so the two `init_scaffold_*` test
//! files stay under the workspace 500-line file cap without
//! duplicating the spawn/assertion plumbing. Not a test target of
//! its own — every item is `pub(crate)` for the includers.

#![allow(
    dead_code,
    reason = "items are used by the #[path] includers, not this file"
)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use std::fs;
use std::path::Path;

use assert_cmd::Command as AssertCommand;

/// Spawn the `cargo-evidence` binary in `cwd`.
pub(crate) fn cargo_evidence(cwd: &Path) -> AssertCommand {
    #[allow(deprecated)]
    let mut cmd = AssertCommand::cargo_bin("cargo-evidence").unwrap();
    cmd.current_dir(cwd);
    cmd
}

/// Every file in init's managed template set, relative to the
/// workspace root. `--force` rewrites exactly these; everything
/// else under `cert/` is user evidence init never touches.
pub(crate) const MANAGED_FILES: &[&str] = &[
    "cert/boundary.toml",
    "cert/floors.toml",
    "cert/profiles/dev.toml",
    "cert/profiles/cert.toml",
    "cert/profiles/record.toml",
    "cert/trace/sys.toml",
    "cert/trace/hlr.toml",
    "cert/trace/llr.toml",
    "cert/trace/tests.toml",
    "cert/trace/derived.toml",
];

/// Parse a jsonl stdout capture into one `serde_json::Value` per
/// non-empty line.
pub(crate) fn parse_jsonl(stdout: &[u8]) -> Vec<serde_json::Value> {
    String::from_utf8_lossy(stdout)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("each jsonl line parses"))
        .collect()
}

/// Assert no trace file under `dir` carries a live (uncommented)
/// `[[requirements]]` or `[[tests]]` table — the non-evidence
/// guarantee: nothing placeholder can enter a bundle. Missing
/// files are skipped (a bundle copies only the layers the bundle
/// format tracks).
pub(crate) fn assert_no_live_entry_tables(trace_dir: &Path) {
    for name in [
        "sys.toml",
        "hlr.toml",
        "llr.toml",
        "tests.toml",
        "derived.toml",
    ] {
        let path = trace_dir.join(name);
        if !path.exists() {
            continue;
        }
        let body = fs::read_to_string(&path).expect("read trace file");
        for (lineno, line) in body.lines().enumerate() {
            let trimmed = line.trim_start();
            assert!(
                !trimmed.starts_with("[[requirements]]") && !trimmed.starts_with("[[tests]]"),
                "{name}:{} carries a live entry table — scaffold example content \
                 must stay commented out:\n{line}",
                lineno + 1
            );
        }
    }
}
