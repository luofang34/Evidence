//! End-to-end tests for `cargo evidence verify` runtime-fault paths
//! that span every output format.
//!
//! Schema Rule 1 (HLR-001) requires that any user-visible verify run
//! emits exactly one terminal — including runtime faults like a
//! missing `--verify-key` file. This test file pins the cross-format
//! contract:
//!
//! | Format    | On unreadable verify-key | Exit | Stdout / stderr shape                        |
//! |-----------|--------------------------|------|----------------------------------------------|
//! | `jsonl`   | finding + terminal pair  | 1    | `VERIFY_RUNTIME_READ_VERIFY_KEY` + `VERIFY_ERROR` |
//! | `json`    | wrapped failure envelope | 1    | `VerifyOutput { success: false, ... }`       |
//! | `human`   | stderr error line        | 1    | `error: reading verify key from <path>`      |
//!
//! The pre-existing `verify_jsonl.rs` covers the bundle-not-found
//! runtime path; this file covers the verify-key runtime path.

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
fn jsonl_emits_terminal_on_unreadable_verify_key() {
    let tmp = TempDir::new().unwrap();
    let bundle = generate_bundle(tmp.path());
    let missing_key = tmp.path().join("no-such-key");

    let output = cargo_evidence()
        .arg("evidence")
        .arg("--format=jsonl")
        .arg("verify")
        .arg(&bundle)
        .arg("--verify-key")
        .arg(&missing_key)
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(1),
        "verify-key I/O fault must map to exit 1; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    let lines = parse_jsonl(&output.stdout);
    assert!(
        lines.len() >= 2,
        "expected at least one finding + the terminal; got {}",
        lines.len(),
    );

    let finding = &lines[lines.len() - 2];
    assert_eq!(
        finding.get("code").and_then(Value::as_str),
        Some("VERIFY_RUNTIME_READ_VERIFY_KEY"),
        "finding line must carry VERIFY_RUNTIME_READ_VERIFY_KEY",
    );
    let loc = finding
        .get("location")
        .expect("ReadVerifyKey diag has location");
    let file = loc
        .get("file")
        .and_then(Value::as_str)
        .expect("location.file is set");
    assert!(
        file.contains("no-such-key"),
        "location.file must echo the user-supplied key path: {:?}",
        file
    );

    let terminal = lines.last().unwrap();
    assert_eq!(
        terminal.get("code").and_then(Value::as_str),
        Some("VERIFY_ERROR"),
    );
    assert_eq!(
        terminal.get("severity").and_then(Value::as_str),
        Some("error")
    );
}

#[test]
fn json_wraps_verify_key_io_failure() {
    let tmp = TempDir::new().unwrap();
    let bundle = generate_bundle(tmp.path());
    let missing_key = tmp.path().join("no-such-key");

    let output = cargo_evidence()
        .arg("evidence")
        .arg("--format=json")
        .arg("verify")
        .arg(&bundle)
        .arg("--verify-key")
        .arg(&missing_key)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));

    let stdout = std::str::from_utf8(&output.stdout).expect("utf8");
    let envelope: Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("stdout must be a single JSON object: {} — {}", stdout, e));

    assert_eq!(
        envelope.get("success").and_then(Value::as_bool),
        Some(false)
    );
    let checks = envelope
        .get("checks")
        .and_then(Value::as_array)
        .expect("checks array");
    assert!(
        checks.iter().any(|c| {
            c.get("name").and_then(Value::as_str) == Some("verify_key")
                && c.get("status").and_then(Value::as_str) == Some("fail")
        }),
        "expected a verify_key/fail check in the envelope: {:?}",
        envelope
    );
    let err_msg = envelope
        .get("error")
        .and_then(Value::as_str)
        .expect("error message");
    assert!(
        err_msg.contains("reading verify key"),
        "error message must name the verify-key read: {:?}",
        err_msg
    );
}

#[test]
fn human_prints_error_on_unreadable_verify_key() {
    let tmp = TempDir::new().unwrap();
    let bundle = generate_bundle(tmp.path());
    let missing_key = tmp.path().join("no-such-key");

    let output = cargo_evidence()
        .arg("evidence")
        .arg("verify")
        .arg(&bundle)
        .arg("--verify-key")
        .arg(&missing_key)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("reading verify key"),
        "stderr must name the verify-key read: {:?}",
        stderr
    );
}
