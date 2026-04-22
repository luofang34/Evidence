//! End-to-end smoke test for `init → trace --backfill-uuids →
//! trace --validate`. Pins the out-of-box contract: init
//! templates use `"SYS-001"`-style placeholder UIDs; a first
//! backfill rewrites both the placeholder `uid` fields and the
//! cross-file `traces_to` references in one pass, so the
//! subsequent validate run succeeds.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use std::path::Path;
use std::process::Command;

use assert_cmd::Command as AssertCommand;
use tempfile::TempDir;

fn cargo_evidence(cwd: &Path) -> AssertCommand {
    #[allow(deprecated)]
    let mut cmd = AssertCommand::cargo_bin("cargo-evidence").unwrap();
    cmd.current_dir(cwd);
    cmd
}

fn seed_git_repo(dir: &Path) {
    // The init / validate flow doesn't strictly require git, but
    // downstream readers of trace_roots expect a usable working
    // directory. Initialize an empty repo with one commit so
    // anything that queries git state gets consistent answers.
    Command::new("git")
        .current_dir(dir)
        .arg("init")
        .arg("-q")
        .output()
        .expect("git init");
    Command::new("git")
        .current_dir(dir)
        .args(["config", "user.email", "test@example.com"])
        .output()
        .expect("git config email");
    Command::new("git")
        .current_dir(dir)
        .args(["config", "user.name", "tester"])
        .output()
        .expect("git config name");
    Command::new("git")
        .current_dir(dir)
        .args(["commit", "--allow-empty", "-q", "-m", "seed"])
        .output()
        .expect("git commit");
}

/// Fresh `cargo evidence init` produces templates that `trace
/// --validate` rejects until `trace --backfill-uuids` runs. After
/// backfill, validate returns clean (VERIFY_OK terminal, exit 0).
#[test]
fn init_then_backfill_then_validate_is_clean() {
    let tmp = TempDir::new().expect("tempdir");
    seed_git_repo(tmp.path());

    cargo_evidence(tmp.path())
        .args(["evidence", "init"])
        .assert()
        .success();

    // Pre-backfill: validate fails because templates hold
    // placeholder "SYS-001" / "HLR-001" / "LLR-001" / "TST-001"
    // uids, which aren't valid UUIDs.
    let pre = cargo_evidence(tmp.path())
        .args(["evidence", "trace", "--validate", "--format=jsonl"])
        .output()
        .expect("spawn");
    assert_ne!(
        pre.status.code(),
        Some(0),
        "pre-backfill validate must fail; stdout:\n{}",
        String::from_utf8_lossy(&pre.stdout)
    );

    // Run backfill. Widened semantic replaces invalid uids +
    // rewrites traces_to references in a single pass.
    cargo_evidence(tmp.path())
        .args(["evidence", "trace", "--backfill-uuids"])
        .assert()
        .success();

    // Post-backfill: validate returns clean.
    let post = cargo_evidence(tmp.path())
        .args(["evidence", "trace", "--validate", "--format=jsonl"])
        .output()
        .expect("spawn");
    assert_eq!(
        post.status.code(),
        Some(0),
        "post-backfill validate must succeed; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&post.stdout),
        String::from_utf8_lossy(&post.stderr)
    );
    // Terminal is VERIFY_OK on the trace subcommand.
    let stdout = String::from_utf8_lossy(&post.stdout);
    let last_nonempty = stdout.lines().rev().find(|l| !l.trim().is_empty()).unwrap();
    assert!(
        last_nonempty.contains("VERIFY_OK"),
        "expected VERIFY_OK terminal; last line was:\n{last_nonempty}"
    );
}

/// Idempotency: running backfill a second time on a tree with
/// all-valid UUIDs is a no-op (writes nothing, reports 0
/// assignments via exit 0).
#[test]
fn second_backfill_is_noop() {
    let tmp = TempDir::new().expect("tempdir");
    seed_git_repo(tmp.path());

    cargo_evidence(tmp.path())
        .args(["evidence", "init"])
        .assert()
        .success();
    cargo_evidence(tmp.path())
        .args(["evidence", "trace", "--backfill-uuids"])
        .assert()
        .success();

    // Capture hashes after first backfill.
    let first = std::fs::read_to_string(tmp.path().join("cert").join("trace").join("hlr.toml"))
        .expect("read hlr.toml");

    // Second backfill.
    cargo_evidence(tmp.path())
        .args(["evidence", "trace", "--backfill-uuids"])
        .assert()
        .success();

    let second = std::fs::read_to_string(tmp.path().join("cert").join("trace").join("hlr.toml"))
        .expect("read hlr.toml");
    assert_eq!(
        first, second,
        "second backfill must not modify the already-valid trace file"
    );
}
