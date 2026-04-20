//! Integration test for `scripts/floors-lower-lint.sh` (//! LLR-037 / TEST-037).
//!
//! The script reads base-vs-HEAD `cert/floors.toml`, finds floor
//! decreases, and requires a `Lower-Floor: <dimension> <reason>`
//! line in the PR body (or commit message) for each. Without the
//! line, the script emits FLOORS_LOWERED_WITHOUT_JUSTIFICATION and
//! exits 1.
//!
//! Three scenarios pinned:
//!
//! 1. Decrease without justification → exit 1 + error message.
//! 2. Decrease WITH justification line → exit 0.
//! 3. No decrease (raise or equal) → exit 0 regardless of body.
//!
//! The script supports `FLOORS_BASE_CONTENT` / `FLOORS_HEAD_CONTENT`
//! env vars that skip the git read, which is what these tests use —
//! no tempdir git repo needed. Those same env vars also let the
//! script work in the Nix sandbox (no git binary available).
//!
//! **Platform**: this module is Linux/macOS only. Windows doesn't
//! ship `bash` out of the box — `Command::new("bash")` triggers the
//! WSL stub which fails — and the lint is a CI gate run on Linux.
//! Windows developers have full test coverage via every other
//! integration-test binary.

#![cfg(not(target_os = "windows"))]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use std::path::PathBuf;
use std::process::Command;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

fn run_lint(base: &str, head: &str, body: &str) -> std::process::Output {
    let script = workspace_root()
        .join("scripts")
        .join("floors-lower-lint.sh");
    Command::new("bash")
        .arg(&script)
        .env("FLOORS_BASE_CONTENT", base)
        .env("FLOORS_HEAD_CONTENT", head)
        .env("PR_BODY", body)
        .current_dir(workspace_root())
        .output()
        .expect("spawn bash")
}

/// Decrease without a Lower-Floor: line fires the lint.
#[test]
fn refuses_decrease_without_justification_line() {
    let base = "[floors]\ndiagnostic_codes = 82\n";
    let head = "[floors]\ndiagnostic_codes = 70\n";
    let out = run_lint(base, head, "a PR body that forgot to justify");

    assert_eq!(
        out.status.code(),
        Some(1),
        "unjustified decrease must exit 1; stdout={}\nstderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    assert!(
        combined.contains("FLOORS_LOWERED_WITHOUT_JUSTIFICATION"),
        "expected FLOORS_LOWERED_WITHOUT_JUSTIFICATION in output; got:\n{}",
        combined
    );
    assert!(
        combined.contains("diagnostic_codes"),
        "expected dimension name in output; got:\n{}",
        combined
    );
}

/// Decrease WITH the justification line passes.
#[test]
fn accepts_decrease_with_justification_line() {
    let base = "[floors]\ndiagnostic_codes = 82\n";
    let head = "[floors]\ndiagnostic_codes = 70\n";
    let body = "Some summary.\n\nLower-Floor: diagnostic_codes consolidating redundant codes.\n";
    let out = run_lint(base, head, body);

    assert!(
        out.status.success(),
        "justified decrease must exit 0; stdout={}\nstderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
}

/// PR bodies in the wild contain backticks, `$()`, angle brackets,
/// and markdown links. The script must still match the
/// `Lower-Floor:` line without expanding any of these as shell
/// code. Pins the env-var transport is byte-exact, not eval'd.
#[test]
fn justification_survives_special_chars_in_pr_body() {
    let base = "[floors]\ndiagnostic_codes = 82\n";
    let head = "[floors]\ndiagnostic_codes = 70\n";
    // Body mixes the justification line with a bunch of shell-
    // hostile content: backticks, command-substitution tokens,
    // angle-bracket tags, markdown links, a literal `$` line.
    let body = r#"## Summary

Consolidating redundant diagnostic codes — we had `$(whoami)` as
a placeholder in the comment, plus a <script> tag and [a link](
https://example.com "quoted title") that shouldn't disturb parsing.

Also see `ls -la | grep foo` in the README for another example.

Lower-Floor: diagnostic_codes consolidated redundant codes post-refactor $(echo ok).
"#;
    let out = run_lint(base, head, body);

    assert!(
        out.status.success(),
        "justified decrease must exit 0 even with special chars; \
         stdout={}\nstderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
}

/// Raising a floor (no decrease) passes without needing a line.
#[test]
fn raise_passes_without_justification() {
    let base = "[floors]\ndiagnostic_codes = 82\n";
    let head = "[floors]\ndiagnostic_codes = 90\n";
    let out = run_lint(base, head, "");

    assert!(
        out.status.success(),
        "raise must exit 0; stdout={}\nstderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    let combined = String::from_utf8_lossy(&out.stdout);
    assert!(
        combined.contains("no floor decreases"),
        "expected 'no floor decreases' in stdout; got: {}",
        combined
    );
}
