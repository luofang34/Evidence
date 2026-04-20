//! Integration tests for
//! `scripts/deterministic-baseline-override-lint.sh` (TEST-045).
//!
//! The script is a Bash gate that compares two
//! `deterministic_hash` values — the prior-main bundle's and the
//! current build's — and, on mismatch, requires a
//! `Override-Deterministic-Baseline:` line in the PR body or head
//! commit message. These tests spawn the script with synthesized
//! `index.json` fixtures and environment variables, matching the
//! shape the CI job sets up.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use std::path::PathBuf;
use std::process::Command;

use tempfile::TempDir;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

/// Absolute path to the lint script.
fn lint_script() -> PathBuf {
    workspace_root()
        .join("scripts")
        .join("deterministic-baseline-override-lint.sh")
}

/// Write a minimal `index.json` carrying a given `deterministic_hash`.
/// The bundle gate only reads the `deterministic_hash` field; other
/// fields are ignored, so the fixture stays tiny.
fn write_index(dir: &TempDir, name: &str, hash: &str) -> PathBuf {
    let path = dir.path().join(name);
    std::fs::write(&path, format!(r#"{{"deterministic_hash":"{}"}}"#, hash))
        .expect("write fixture");
    path
}

/// Run the lint script with the given env vars and return
/// `(exit_code, stderr)`.
fn run_lint(
    prior: &PathBuf,
    current: &PathBuf,
    pr_body: Option<&str>,
    commit_msg: Option<&str>,
) -> (i32, String) {
    let mut cmd = Command::new(lint_script());
    cmd.arg(prior).arg(current);
    if let Some(body) = pr_body {
        cmd.env("PR_BODY", body);
    } else {
        cmd.env_remove("PR_BODY");
    }
    if let Some(msg) = commit_msg {
        cmd.env("COMMIT_MESSAGE", msg);
    } else {
        cmd.env_remove("COMMIT_MESSAGE");
    }
    let out = cmd.output().expect("spawn lint script");
    let code = out.status.code().unwrap_or(-1);
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    (code, stderr)
}

/// Positive match: identical hashes on both sides, no override line,
/// exit 0 silently.
#[test]
fn match_exit_zero() {
    let tmp = TempDir::new().expect("tempdir");
    let hash = "deadbeefcafef00dc0ffee00000000000000000000000000000000000000d00d";
    let prior = write_index(&tmp, "prior.json", hash);
    let current = write_index(&tmp, "current.json", hash);
    let (code, stderr) = run_lint(&prior, &current, None, None);
    assert_eq!(code, 0, "match should exit 0; stderr:\n{}", stderr);
}

/// Silent drift: hashes differ, no override, exit 1 with both
/// hashes in the stderr payload.
#[test]
fn drift_without_override_fails() {
    let tmp = TempDir::new().expect("tempdir");
    let prior = write_index(&tmp, "prior.json", "a".repeat(64).as_str());
    let current = write_index(&tmp, "current.json", "b".repeat(64).as_str());
    let (code, stderr) = run_lint(&prior, &current, Some(""), None);
    assert_eq!(code, 1, "drift should exit 1; stderr:\n{}", stderr);
    assert!(
        stderr.contains("SILENT DRIFT DETECTED"),
        "stderr must surface the drift headline; got:\n{}",
        stderr
    );
    assert!(
        stderr.contains(&"a".repeat(64)),
        "stderr must print prior hash; got:\n{}",
        stderr
    );
    assert!(
        stderr.contains(&"b".repeat(64)),
        "stderr must print current hash; got:\n{}",
        stderr
    );
    assert!(
        stderr.contains("Override-Deterministic-Baseline:"),
        "stderr must hand the user the override syntax; got:\n{}",
        stderr
    );
}

/// Override in PR body: hashes differ but the body contains
/// `Override-Deterministic-Baseline: <reason>`; exit 0 with an
/// accepting stderr log.
#[test]
fn drift_with_override_passes() {
    let tmp = TempDir::new().expect("tempdir");
    let prior = write_index(&tmp, "prior.json", "a".repeat(64).as_str());
    let current = write_index(&tmp, "current.json", "b".repeat(64).as_str());
    let body = "## Summary\n\nBumped serde_json.\n\nOverride-Deterministic-Baseline: dep bump to fix CVE-NNNN-NNNN\n";
    let (code, stderr) = run_lint(&prior, &current, Some(body), None);
    assert_eq!(
        code, 0,
        "drift + override should exit 0; stderr:\n{}",
        stderr
    );
    assert!(
        stderr.contains("accepting"),
        "stderr should note acceptance; got:\n{}",
        stderr
    );
}

/// Override in push-mode commit message: identical contract, but
/// `PR_BODY` empty and `COMMIT_MESSAGE` carries the line. Exit 0.
#[test]
fn drift_with_override_in_commit_message_passes() {
    let tmp = TempDir::new().expect("tempdir");
    let prior = write_index(&tmp, "prior.json", "a".repeat(64).as_str());
    let current = write_index(&tmp, "current.json", "b".repeat(64).as_str());
    let msg = "chore: bump deps\n\nOverride-Deterministic-Baseline: workspace-wide dep upgrade\n";
    let (code, stderr) = run_lint(&prior, &current, Some(""), Some(msg));
    assert_eq!(
        code, 0,
        "push-mode override should exit 0; stderr:\n{}",
        stderr
    );
}

/// Missing prior bundle: the prior main-branch artifact expired
/// (14-day retention) or simply never existed for a fresh repo.
/// Script degrades to a logged skip + exit 0 so the gate stays
/// best-effort.
#[test]
fn missing_prior_artifact_is_skip() {
    let tmp = TempDir::new().expect("tempdir");
    let missing = tmp.path().join("prior-does-not-exist.json");
    let current = write_index(&tmp, "current.json", "a".repeat(64).as_str());
    let (code, stderr) = run_lint(&missing, &current, None, None);
    assert_eq!(
        code, 0,
        "missing prior artifact should exit 0 (degraded skip); stderr:\n{}",
        stderr
    );
    assert!(
        stderr.contains("skipping"),
        "stderr should log the skip; got:\n{}",
        stderr
    );
}

/// Regex precision: a `Override-Deterministic-Baseline:` line with
/// no reason body must NOT count as an override. The regex
/// `^Override-Deterministic-Baseline: .+` requires at least one
/// non-space character after the colon + space.
#[test]
fn override_without_reason_does_not_pass() {
    let tmp = TempDir::new().expect("tempdir");
    let prior = write_index(&tmp, "prior.json", "a".repeat(64).as_str());
    let current = write_index(&tmp, "current.json", "b".repeat(64).as_str());
    let body = "## Summary\n\nOverride-Deterministic-Baseline:\n";
    let (code, _stderr) = run_lint(&prior, &current, Some(body), None);
    assert_eq!(
        code, 1,
        "override with empty reason must still fail the silent-drift gate"
    );
}

/// Malformed input: current bundle missing `deterministic_hash`
/// field. Exit 2 (invocation error, not drift).
#[test]
fn malformed_current_bundle_exit_two() {
    let tmp = TempDir::new().expect("tempdir");
    let prior = write_index(&tmp, "prior.json", "a".repeat(64).as_str());
    let current = tmp.path().join("current.json");
    std::fs::write(&current, r#"{"other_field":"x"}"#).expect("write fixture");
    let (code, stderr) = run_lint(&prior, &current, None, None);
    assert_eq!(
        code, 2,
        "malformed bundle (missing field) must exit 2; stderr:\n{}",
        stderr
    );
    assert!(
        stderr.contains("deterministic_hash"),
        "stderr should name the missing field; got:\n{}",
        stderr
    );
}
