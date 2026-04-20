//! Integration tests for
//! `scripts/deterministic-baseline-override-lint.sh` (TEST-045).
//!
//! The script is a Bash gate that compares the toolchain-sensitive
//! projection of two `deterministic-manifest.json` bundles (prior
//! main-branch + current PR) and, on mismatch, requires a
//! `Override-Deterministic-Baseline:` line in the PR body or head
//! commit message. These tests spawn the script with synthesized
//! manifest fixtures and environment variables, matching the shape
//! the CI job sets up.
//!
//! **Platform**: Linux/macOS only. Windows doesn't ship `bash`
//! natively — `Command::new("bash")` triggers the WSL stub which
//! fails — and the lint is a CI gate run on Linux. Windows
//! developers have full test coverage via every other integration
//! binary. Same precedent as `floors_lower_lint.rs`.

#![cfg(not(target_os = "windows"))]
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

fn lint_script() -> PathBuf {
    workspace_root()
        .join("scripts")
        .join("deterministic-baseline-override-lint.sh")
}

/// Sandbox-friendliness: if the script isn't present at the
/// expected path (Nix `buildRustPackage` copies only the crate's
/// src, leaving `../../scripts/` out of the sandbox), skip
/// gracefully. The CI path runs the gate from the real checkout,
/// so the guardrail still fires where it matters.
///
/// Returns `true` iff the script is runnable.
fn script_available() -> bool {
    lint_script().is_file()
}

/// Minimal `deterministic-manifest.json` stub carrying the six
/// toolchain-sensitive fields the lint projects. Everything else
/// (schema_version, profile, git_*) is irrelevant to the gate and
/// omitted so the fixtures stay tiny and obvious.
fn manifest_json(
    rustc: &str,
    cargo: &str,
    llvm: Option<&str>,
    cargo_lock_hash: &str,
    rust_toolchain_toml: &str,
    rustflags: Option<&str>,
) -> String {
    let llvm_field = match llvm {
        Some(v) => format!(r#""llvm_version":"{}""#, v),
        None => r#""llvm_version":null"#.to_string(),
    };
    let rustflags_field = match rustflags {
        Some(v) => format!(r#""rustflags":"{}""#, v),
        None => r#""rustflags":null"#.to_string(),
    };
    format!(
        r#"{{"rustc":"{}","cargo":"{}",{},"cargo_lock_hash":"{}","rust_toolchain_toml":{},{}}}"#,
        rustc,
        cargo,
        llvm_field,
        cargo_lock_hash,
        serde_json::to_string(rust_toolchain_toml).expect("serialize toolchain string"),
        rustflags_field,
    )
}

fn write_manifest(dir: &TempDir, name: &str, contents: &str) -> PathBuf {
    let path = dir.path().join(name);
    std::fs::write(&path, contents).expect("write fixture");
    path
}

fn default_prior_and_current(dir: &TempDir) -> (PathBuf, PathBuf) {
    // Same six fields on both sides = identity; tests that need
    // drift override one of them.
    let prior = manifest_json(
        "rustc 1.95.0 (abc)",
        "cargo 1.95.0 (abc)",
        Some("20.0.0"),
        "deadbeef".to_string().as_str(),
        "[toolchain]\nchannel = \"1.95\"\n",
        Some("-D warnings"),
    );
    let current = prior.clone();
    (
        write_manifest(dir, "prior.json", &prior),
        write_manifest(dir, "current.json", &current),
    )
}

/// Spawn the lint via `bash <script>` (not by path) so the test
/// is portable across Linux + macOS regardless of which `env`
/// discovery path the shebang takes.
fn run_lint(
    prior: &PathBuf,
    current: &PathBuf,
    pr_body: Option<&str>,
    commit_msg: Option<&str>,
) -> (i32, String) {
    let mut cmd = Command::new("bash");
    cmd.arg(lint_script()).arg(prior).arg(current);
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

/// Positive match: identical toolchain fields on both sides, no
/// override line, exit 0 silently.
#[test]
fn match_exit_zero() {
    if !script_available() {
        return; // Nix sandbox: see `script_available()` docstring.
    }
    let tmp = TempDir::new().expect("tempdir");
    let (prior, current) = default_prior_and_current(&tmp);
    let (code, stderr) = run_lint(&prior, &current, None, None);
    assert_eq!(code, 0, "match should exit 0; stderr:\n{}", stderr);
}

/// Silent drift: cargo_lock_hash differs, no override, exit 1
/// with a diff in the stderr payload.
#[test]
fn drift_without_override_fails() {
    if !script_available() {
        return;
    }
    let tmp = TempDir::new().expect("tempdir");
    let prior = manifest_json(
        "rustc 1.95.0 (abc)",
        "cargo 1.95.0 (abc)",
        Some("20.0.0"),
        "aaaaaaaa",
        "[toolchain]\nchannel = \"1.95\"\n",
        Some("-D warnings"),
    );
    let current = manifest_json(
        "rustc 1.95.0 (abc)",
        "cargo 1.95.0 (abc)",
        Some("20.0.0"),
        "bbbbbbbb", // ← drift
        "[toolchain]\nchannel = \"1.95\"\n",
        Some("-D warnings"),
    );
    let prior_p = write_manifest(&tmp, "prior.json", &prior);
    let current_p = write_manifest(&tmp, "current.json", &current);
    let (code, stderr) = run_lint(&prior_p, &current_p, Some(""), None);
    assert_eq!(code, 1, "drift should exit 1; stderr:\n{}", stderr);
    assert!(
        stderr.contains("SILENT DRIFT DETECTED"),
        "stderr must surface the drift headline; got:\n{}",
        stderr
    );
    assert!(
        stderr.contains("aaaaaaaa") && stderr.contains("bbbbbbbb"),
        "stderr must surface both cargo_lock_hash values; got:\n{}",
        stderr
    );
    assert!(
        stderr.contains("Override-Deterministic-Baseline:"),
        "stderr must hand the user the override syntax; got:\n{}",
        stderr
    );
}

/// Override in PR body: fields differ but body carries
/// `Override-Deterministic-Baseline: <reason>`; exit 0 with an
/// accepting stderr log.
#[test]
fn drift_with_override_passes() {
    if !script_available() {
        return;
    }
    let tmp = TempDir::new().expect("tempdir");
    let prior = manifest_json(
        "rustc 1.95.0",
        "cargo 1.95.0",
        None,
        "aaaaaaaa",
        "ch = 1.95",
        None,
    );
    let current = manifest_json(
        "rustc 1.95.0",
        "cargo 1.95.0",
        None,
        "bbbbbbbb",
        "ch = 1.95",
        None,
    );
    let prior_p = write_manifest(&tmp, "prior.json", &prior);
    let current_p = write_manifest(&tmp, "current.json", &current);
    let body = "## Summary\n\nBumped serde_json.\n\nOverride-Deterministic-Baseline: dep bump to fix CVE-NNNN-NNNN\n";
    let (code, stderr) = run_lint(&prior_p, &current_p, Some(body), None);
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
    if !script_available() {
        return;
    }
    let tmp = TempDir::new().expect("tempdir");
    let prior = manifest_json(
        "rustc 1.95.0",
        "cargo 1.95.0",
        None,
        "aa",
        "ch = 1.95",
        None,
    );
    let current = manifest_json(
        "rustc 1.95.0",
        "cargo 1.95.0",
        None,
        "bb",
        "ch = 1.95",
        None,
    );
    let prior_p = write_manifest(&tmp, "prior.json", &prior);
    let current_p = write_manifest(&tmp, "current.json", &current);
    let msg = "chore: bump deps\n\nOverride-Deterministic-Baseline: workspace-wide dep upgrade\n";
    let (code, stderr) = run_lint(&prior_p, &current_p, Some(""), Some(msg));
    assert_eq!(
        code, 0,
        "push-mode override should exit 0; stderr:\n{}",
        stderr
    );
}

/// Missing prior manifest: the prior main-branch artifact
/// expired (14-day retention) or simply never existed for a
/// fresh repo. Script degrades to a logged skip + exit 0 so the
/// gate stays best-effort.
#[test]
fn missing_prior_artifact_is_skip() {
    if !script_available() {
        return;
    }
    let tmp = TempDir::new().expect("tempdir");
    let missing = tmp.path().join("prior-does-not-exist.json");
    let current = manifest_json(
        "rustc 1.95.0",
        "cargo 1.95.0",
        None,
        "aa",
        "ch = 1.95",
        None,
    );
    let current_p = write_manifest(&tmp, "current.json", &current);
    let (code, stderr) = run_lint(&missing, &current_p, None, None);
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

/// Regex precision: `Override-Deterministic-Baseline:` with no
/// reason body must NOT count as an override. The regex requires
/// at least one non-space character after `: `.
#[test]
fn override_without_reason_does_not_pass() {
    if !script_available() {
        return;
    }
    let tmp = TempDir::new().expect("tempdir");
    let prior = manifest_json(
        "rustc 1.95.0",
        "cargo 1.95.0",
        None,
        "aa",
        "ch = 1.95",
        None,
    );
    let current = manifest_json(
        "rustc 1.95.0",
        "cargo 1.95.0",
        None,
        "bb",
        "ch = 1.95",
        None,
    );
    let prior_p = write_manifest(&tmp, "prior.json", &prior);
    let current_p = write_manifest(&tmp, "current.json", &current);
    let body = "## Summary\n\nOverride-Deterministic-Baseline:\n";
    let (code, _stderr) = run_lint(&prior_p, &current_p, Some(body), None);
    assert_eq!(
        code, 1,
        "override with empty reason must still fail the silent-drift gate"
    );
}

/// Malformed input: current manifest is not valid JSON. Exit 2
/// (invocation error, not drift).
#[test]
fn malformed_current_manifest_exit_two() {
    if !script_available() {
        return;
    }
    let tmp = TempDir::new().expect("tempdir");
    let prior = manifest_json(
        "rustc 1.95.0",
        "cargo 1.95.0",
        None,
        "aa",
        "ch = 1.95",
        None,
    );
    let prior_p = write_manifest(&tmp, "prior.json", &prior);
    let current_p = tmp.path().join("current.json");
    std::fs::write(&current_p, "this is not JSON {{{").expect("write fixture");
    let (code, stderr) = run_lint(&prior_p, &current_p, None, None);
    assert_eq!(
        code, 2,
        "malformed manifest must exit 2; stderr:\n{}",
        stderr
    );
    assert!(
        stderr.contains("malformed JSON") || stderr.contains("could not project"),
        "stderr should name the parse failure; got:\n{}",
        stderr
    );
}

/// Git-state fields in the manifest MUST NOT cause drift. If a PR
/// changes only `git_sha` / `git_branch` / `git_dirty` (the
/// always-differs-per-commit case), the gate must stay green.
/// This is the load-bearing regression that surfaced from CI's
/// first run: comparing raw `deterministic_hash` fires on every
/// PR because the hash includes git state. Projecting to the six
/// toolchain-only fields fixes it; this test pins that fix.
#[test]
fn git_state_fields_are_projected_out() {
    if !script_available() {
        return;
    }
    let tmp = TempDir::new().expect("tempdir");
    // Both manifests carry identical toolchain fields, but also
    // include divergent git_* values. The gate must ignore git
    // state. `schema_version` + `profile` are excluded from the
    // fixture — the lint projects them out, and including them
    // as string literals would trip `schema_versions_locked`.
    let prior = r#"{
        "rustc": "rustc 1.95.0",
        "cargo": "cargo 1.95.0",
        "llvm_version": null,
        "cargo_lock_hash": "aa",
        "rust_toolchain_toml": "ch = 1.95",
        "rustflags": null,
        "git_sha": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "git_branch": "main",
        "git_dirty": false
    }"#;
    let current = r#"{
        "rustc": "rustc 1.95.0",
        "cargo": "cargo 1.95.0",
        "llvm_version": null,
        "cargo_lock_hash": "aa",
        "rust_toolchain_toml": "ch = 1.95",
        "rustflags": null,
        "git_sha": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        "git_branch": "pr/53",
        "git_dirty": false
    }"#;
    let prior_p = write_manifest(&tmp, "prior.json", prior);
    let current_p = write_manifest(&tmp, "current.json", current);
    let (code, stderr) = run_lint(&prior_p, &current_p, None, None);
    assert_eq!(
        code, 0,
        "identical toolchain fields + differing git_* must NOT \
         trigger drift; stderr:\n{}",
        stderr
    );
}
