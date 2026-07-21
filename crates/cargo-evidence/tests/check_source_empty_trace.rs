//! Empty-trace fail-closed regression for `check --mode=source`
//! (LLR-105). `check` over a workspace whose trace root
//! exists but holds zero requirements must not terminate
//! `VERIFY_OK` with "0 requirement(s) satisfied" — a success
//! terminal over no evidence. The run must emit the typed
//! `TRACE_EVIDENCE_EMPTY` diagnostic and terminate `VERIFY_FAIL`
//! with a non-zero exit. Standalone file (not a `#[path]` sibling
//! of `check_source_correctness.rs`) so each side stays under the
//! 500-line workspace limit.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use std::fs;
use std::path::Path;

use assert_cmd::Command;
use evidence_core::schema_versions::TRACE;
use serde_json::Value;
use tempfile::TempDir;

fn cargo_evidence() -> Command {
    #[allow(deprecated)]
    Command::cargo_bin("cargo-evidence").unwrap()
}

/// Seed a minimal Rust workspace (Cargo.toml + src/lib.rs) at
/// `dir`. Empty library with no tests satisfies libtest:
/// "test result: ok. 0 passed" — enough for `cmd_check_source`
/// to reach the trace-validation phase. Mirrors the helper in
/// `check_source_correctness.rs`; inlined to keep this file
/// standalone.
fn seed_minimal_cargo_workspace(dir: &Path) {
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("Cargo.toml"),
        r#"[package]
name = "downstream-fixture"
version = "0.1.0"
edition = "2024"

[lib]
path = "src/lib.rs"
"#,
    )
    .unwrap();
    fs::write(dir.join("src/lib.rs"), "// empty library\n").unwrap();
}

/// Zero-requirement trace root → `TRACE_EVIDENCE_EMPTY` +
/// `VERIFY_FAIL` + non-zero exit. Never `VERIFY_OK` over no
/// evidence.
#[test]
fn check_source_empty_trace_fails_closed() {
    let downstream = TempDir::new().expect("tempdir");
    seed_minimal_cargo_workspace(downstream.path());
    let trace_dir = downstream.path().join("cert/trace");
    fs::create_dir_all(&trace_dir).unwrap();
    for (name, list_key) in [
        ("sys.toml", "requirements"),
        ("hlr.toml", "requirements"),
        ("llr.toml", "requirements"),
        ("tests.toml", "tests"),
    ] {
        fs::write(
            trace_dir.join(name),
            format!(
                "{list_key} = []\n\n[schema]\nversion = \"{TRACE}\"\n\n[meta]\ndocument_id = \"DS\"\nrevision = \"1.0\"\n"
            ),
        )
        .unwrap();
    }

    let out = cargo_evidence()
        .args(["evidence", "--format=jsonl", "check", "--mode=source"])
        .arg(downstream.path())
        .output()
        .expect("spawn");

    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert_ne!(
        out.status.code(),
        Some(0),
        "check over zero requirements must not succeed; stdout={stdout}\nstderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let codes: Vec<String> = stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| {
            serde_json::from_str::<Value>(l)
                .ok()
                .and_then(|v| v.get("code").and_then(|c| c.as_str()).map(str::to_string))
        })
        .collect();
    assert!(
        codes.iter().any(|c| c == "TRACE_EVIDENCE_EMPTY"),
        "expected TRACE_EVIDENCE_EMPTY in stream; codes={codes:?}\nstdout={stdout}"
    );
    assert_eq!(
        codes.last().map(String::as_str),
        Some("VERIFY_FAIL"),
        "stream must terminate VERIFY_FAIL, never VERIFY_OK over no evidence; codes={codes:?}"
    );
}

/// Missing trace root → `TRACE_EVIDENCE_NOT_ADOPTED` +
/// `VERIFY_FAIL` + non-zero exit. The root is configured by
/// convention (`cert/trace`) but absent on disk — a distinct
/// adoption state from "present but empty".
#[test]
fn check_source_missing_trace_root_fails_closed() {
    let downstream = TempDir::new().expect("tempdir");
    seed_minimal_cargo_workspace(downstream.path());
    // Deliberately no cert/trace directory.

    let out = cargo_evidence()
        .args(["evidence", "--format=jsonl", "check", "--mode=source"])
        .arg(downstream.path())
        .output()
        .expect("spawn");

    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert_ne!(
        out.status.code(),
        Some(0),
        "check over a missing trace root must not succeed; stdout={stdout}\nstderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let codes: Vec<String> = stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| {
            serde_json::from_str::<Value>(l)
                .ok()
                .and_then(|v| v.get("code").and_then(|c| c.as_str()).map(str::to_string))
        })
        .collect();
    assert!(
        codes.iter().any(|c| c == "TRACE_EVIDENCE_NOT_ADOPTED"),
        "expected TRACE_EVIDENCE_NOT_ADOPTED in stream; codes={codes:?}\nstdout={stdout}"
    );
    assert_eq!(
        codes.last().map(String::as_str),
        Some("VERIFY_FAIL"),
        "stream must terminate VERIFY_FAIL; codes={codes:?}"
    );
}
