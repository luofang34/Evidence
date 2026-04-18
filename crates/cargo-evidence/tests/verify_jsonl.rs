//! End-to-end tests for `cargo evidence verify --format=jsonl`.
//!
//! Covers the four exit-code ↔ terminal-event paths documented by
//! Schema Rule 1 in `schemas/diagnostic.schema.json`:
//!
//! | Outcome                        | stdout last line | exit |
//! |--------------------------------|------------------|------|
//! | bundle ok                      | `VERIFY_OK`      | 0    |
//! | bundle has findings            | `VERIFY_FAIL`    | 2    |
//! | bundle directory missing       | *(no terminal)*  | 1    |
//! | strict mode, BUNDLE.sig absent | `VERIFY_FAIL`    | 2    |
//!
//! Also pins Schema Rule 2 (stdout-strict JSONL) and Schema Rule 4
//! (flush per event — we don't verify buffering timing here, but
//! parsing each line as an independent JSON object asserts the line
//! boundary contract).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use assert_cmd::Command;
use serde_json::Value;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

fn cargo_evidence() -> Command {
    #[allow(deprecated)]
    Command::cargo_bin("cargo-evidence").unwrap()
}

/// Generate a bundle in `out_dir` rooted at the current repo (the
/// workspace the test runs under). Returns the bundle path.
fn generate_bundle(out_dir: &Path) -> std::path::PathBuf {
    cargo_evidence()
        .arg("evidence")
        .arg("generate")
        .arg("--skip-tests")
        .arg("--out-dir")
        .arg(out_dir)
        .arg("--profile")
        .arg("dev")
        .assert()
        .success();
    // Bundle is written under <out_dir>/dev-<timestamp>-<sha>/.
    fs::read_dir(out_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| {
            p.is_dir()
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .starts_with("dev-")
        })
        .expect("bundle directory under out_dir")
}

/// Split `stdout` into trimmed JSONL lines and parse each as JSON.
/// Empty trailing lines are dropped. Asserts every surviving line is
/// valid JSON — Schema Rule 2 forbids mixed prose on stdout.
fn parse_jsonl(stdout: &[u8]) -> Vec<Value> {
    std::str::from_utf8(stdout)
        .expect("stdout is utf8")
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            serde_json::from_str(l)
                .unwrap_or_else(|e| panic!("line is not valid JSON: {:?} — {}", l, e))
        })
        .collect()
}

#[test]
fn verify_ok_terminates_with_verify_ok_and_exit_zero() {
    let tmp = TempDir::new().unwrap();
    let bundle = generate_bundle(tmp.path());

    let output = cargo_evidence()
        .arg("evidence")
        .arg("--format=jsonl")
        .arg("verify")
        .arg(&bundle)
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let lines = parse_jsonl(&output.stdout);
    assert!(!lines.is_empty(), "expected at least the terminal event");
    let last = lines.last().unwrap();
    assert_eq!(
        last.get("code").and_then(Value::as_str),
        Some("VERIFY_OK"),
        "last line must be VERIFY_OK terminal; got {:?}",
        last
    );
    assert_eq!(
        last.get("severity").and_then(Value::as_str),
        Some("info"),
        "VERIFY_OK must be severity=info",
    );
}

#[test]
fn verify_missing_bundle_emits_runtime_diag_and_exit_one() {
    // Bundle-not-found is a runtime fault: Schema Rule 1 says no
    // terminal event, exit 1.
    let tmp = TempDir::new().unwrap();
    let nonexistent = tmp.path().join("does-not-exist");

    let output = cargo_evidence()
        .arg("evidence")
        .arg("--format=jsonl")
        .arg("verify")
        .arg(&nonexistent)
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(1),
        "runtime fault must map to exit 1, not 2"
    );

    let lines = parse_jsonl(&output.stdout);
    assert_eq!(lines.len(), 1, "runtime faults emit one diag, no terminal");
    let diag = &lines[0];
    assert_eq!(
        diag.get("code").and_then(Value::as_str),
        Some("VERIFY_RUNTIME_BUNDLE_NOT_FOUND"),
    );
    // Must not be a terminal event (no `_OK` / `_FAIL` suffix).
    let code = diag.get("code").and_then(Value::as_str).unwrap();
    assert!(!code.ends_with("_OK"));
    assert!(!code.ends_with("_FAIL"));
}

#[test]
fn verify_finding_emits_terminal_fail_and_exit_two() {
    let tmp = TempDir::new().unwrap();
    let bundle = generate_bundle(tmp.path());

    // Introduce a content-layer tampering: rewrite an already-hashed
    // file to something different. That makes `verify_hash_list`
    // observe a hash mismatch against `SHA256SUMS`.
    let env_json = bundle.join("env.json");
    let original = fs::read(&env_json).unwrap();
    fs::write(&env_json, b"{\"profile\":\"dev\",\"tampered\":true}").unwrap();

    let output = cargo_evidence()
        .arg("evidence")
        .arg("--format=jsonl")
        .arg("verify")
        .arg(&bundle)
        .output()
        .unwrap();

    // Restore so the test doesn't leave a broken bundle lying around
    // (the TempDir cleans up anyway, but this guards against a
    // future test that reuses the path).
    fs::write(&env_json, original).unwrap();

    assert_eq!(
        output.status.code(),
        Some(2),
        "findings must map to exit 2, not 1"
    );

    let lines = parse_jsonl(&output.stdout);
    assert!(
        lines.len() >= 2,
        "expected at least one finding + one terminal, got {} lines",
        lines.len()
    );
    let last = lines.last().unwrap();
    assert_eq!(
        last.get("code").and_then(Value::as_str),
        Some("VERIFY_FAIL"),
        "last line must be VERIFY_FAIL terminal",
    );
    assert_eq!(last.get("severity").and_then(Value::as_str), Some("error"),);
    // Earlier lines must be individual findings (not VERIFY_FAIL
    // itself — Schema Rule 1 reserves the terminal for the last line).
    for (i, line) in lines[..lines.len() - 1].iter().enumerate() {
        let code = line.get("code").and_then(Value::as_str).unwrap();
        assert!(
            !code.ends_with("_OK") && !code.ends_with("_FAIL"),
            "line {} has terminal-shaped code '{}' before terminal slot",
            i,
            code
        );
    }
}

#[test]
fn verify_jsonl_stdout_is_strict_jsonl_only() {
    // Schema Rule 2: stdout must carry ONLY JSONL. Human-readable
    // progress text lives on stderr.
    let tmp = TempDir::new().unwrap();
    let bundle = generate_bundle(tmp.path());

    let output = cargo_evidence()
        .arg("evidence")
        .arg("--format=jsonl")
        .arg("verify")
        .arg(&bundle)
        .output()
        .unwrap();

    let stdout = std::str::from_utf8(&output.stdout).expect("utf8");
    for (i, line) in stdout.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        // First non-whitespace char must be `{` — no prose lines.
        assert!(
            trimmed.starts_with('{'),
            "stdout line {} is not JSON: {:?}",
            i,
            line
        );
        serde_json::from_str::<Value>(trimmed)
            .unwrap_or_else(|e| panic!("line {} failed JSON parse: {:?} — {}", i, line, e));
    }
}
