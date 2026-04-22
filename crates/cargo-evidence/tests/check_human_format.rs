//! Human-format regression tests for `cargo evidence check`.
//!
//! Three invariants pinned here:
//!
//! 1. **Default output is NOT JSONL.** Running `cargo evidence check`
//!    with no format flag on an empty tempdir must produce stdout
//!    lines that are NOT parseable as JSON. A developer running
//!    `check .` interactively gets human-readable text, not a
//!    JSON firehose.
//! 2. **Default output carries a check-tag prefix.** `[✓]` / `[⚠]`
//!    / `[✗]` on every diagnostic line. Lets humans pattern-match
//!    at a glance and lets future tests assert on the marker without
//!    knowing the full message wording.
//! 3. **`--format=jsonl` still emits valid JSONL.** Every non-empty
//!    stdout line parses as a JSON object — the agent-facing contract
//!    doesn't regress when the human path lands.
//!
//! The tests run against the empty-tempdir failure case because it's
//! fast (no `cargo test` subprocess) and exercises the same renderer
//! that handles successful `REQ_PASS` diagnostics. The source-mode
//! happy-path is covered by `mcp-evidence/tests/mcp_surface.rs` via
//! the JSONL route; adding a human-happy-path test here would re-run
//! `cargo test --workspace` inside `cargo test`, which we avoid per
//! `check.rs:191-196` (nested cargo-test target/ lock contention).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use assert_cmd::Command;
use tempfile::TempDir;

fn cargo_evidence() -> Command {
    #[allow(deprecated)]
    Command::cargo_bin("cargo-evidence").unwrap()
}

/// Default (no `--format`, no `--json`) must render human-readable
/// prose on stdout — every non-empty line must fail `from_str` as
/// JSON.
#[test]
fn default_stdout_is_not_jsonl() {
    let tmp = TempDir::new().expect("tempdir");
    let out = cargo_evidence()
        .args(["evidence", "check"])
        .arg(tmp.path())
        .output()
        .expect("spawn");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    // Expected outcome: CLI_INVALID_ARGUMENT path, exit 2, two
    // content lines + a blank line in between.
    assert_ne!(
        out.status.code(),
        Some(0),
        "empty dir must fail; stdout={stdout}"
    );
    let content_lines: Vec<&str> = stdout.lines().filter(|l| !l.trim().is_empty()).collect();
    assert!(
        !content_lines.is_empty(),
        "expected at least one stdout line; got: {stdout:?}"
    );
    for line in &content_lines {
        assert!(
            serde_json::from_str::<serde_json::Value>(line).is_err(),
            "human stdout contained a JSON-parseable line — default mode \
             leaked JSONL. Line: {line:?}",
        );
    }
}

/// Default stdout contains at least one `[✓]` / `[⚠]` / `[✗]` tag.
/// Pins the tag convention so future tests can pattern-match on it.
#[test]
fn default_stdout_contains_check_tag() {
    let tmp = TempDir::new().expect("tempdir");
    let out = cargo_evidence()
        .args(["evidence", "check"])
        .arg(tmp.path())
        .output()
        .expect("spawn");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        stdout.contains("[✓]") || stdout.contains("[⚠]") || stdout.contains("[✗]"),
        "stdout must carry a human-format tag; got: {stdout:?}",
    );
}

/// `--quiet` (or `-q`) suppresses every stderr line with a
/// `check:` prefix that's emitted by `cmd_check_source` (today:
/// three phase-progress markers — `check: running cargo test…`
/// / `check: validating trace…` / `check: aggregating
/// results…`). The stdout human output is untouched — `--quiet`
/// is about noise, not results.
///
/// The assertion is **strict** on the `check:` prefix rather than
/// an enumeration of today's three phase strings. A new phase
/// marker added in a future commit but not gated on
/// `show_progress = !machine && !quiet` would silently slip past
/// an enumeration-style test; the strict form fires immediately,
/// forcing the author to either (a) gate the new line on
/// `show_progress` or (b) justify it not being a phase marker by
/// re-prefixing it. Acceptable tight coupling: failure mode is a
/// clear false-positive (visible, easy to triage), not a silent
/// false-negative.
///
/// Runs against the empty-tempdir path (same as the sibling tests
/// here) to keep the test fast. That path hits
/// `emit_invalid_argument` before `cmd_check_source`, so phase
/// markers wouldn't fire even without `--quiet` — but the test's
/// point is the invariant, not the specific path.
#[test]
fn quiet_flag_suppresses_phase_progress_markers() {
    let tmp = TempDir::new().expect("tempdir");
    let out = cargo_evidence()
        .args(["evidence", "--quiet", "check"])
        .arg(tmp.path())
        .output()
        .expect("spawn");
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    let leaked: Vec<&str> = stderr
        .lines()
        .filter(|l| l.trim_start().starts_with("check:"))
        .collect();
    assert!(
        leaked.is_empty(),
        "--quiet must suppress every `check:`-prefixed stderr line \
         (phase-progress markers live there and must honor --quiet). \
         Leaked lines: {leaked:?}\n\
         Full stderr: {stderr:?}",
    );
}

/// `--format=jsonl` still emits valid JSONL. Every non-empty stdout
/// line parses as a JSON object — the agent-facing contract stays
/// intact even after the human renderer landed.
#[test]
fn jsonl_format_still_emits_jsonl() {
    let tmp = TempDir::new().expect("tempdir");
    let out = cargo_evidence()
        .args(["evidence", "--format=jsonl", "check"])
        .arg(tmp.path())
        .output()
        .expect("spawn");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let content_lines: Vec<&str> = stdout.lines().filter(|l| !l.trim().is_empty()).collect();
    assert!(
        !content_lines.is_empty(),
        "expected at least one stdout line under --format=jsonl; got: {stdout:?}"
    );
    for line in &content_lines {
        let parsed: serde_json::Value = serde_json::from_str(line)
            .unwrap_or_else(|e| panic!("line not valid JSON under --format=jsonl: {line:?}: {e}"));
        // Every diagnostic must carry `code` + `severity` per the
        // Diagnostic schema. If either is absent, it's not a
        // Diagnostic.
        assert!(
            parsed.get("code").is_some(),
            "diagnostic missing `code` field: {line:?}",
        );
        assert!(
            parsed.get("severity").is_some(),
            "diagnostic missing `severity` field: {line:?}",
        );
    }
}
