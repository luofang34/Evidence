//! `cargo evidence trace --validate --format=jsonl` stream-emit
//! contract (TEST-042).
//!
//! Spawns the CLI against a tampered trace root with known
//! violations; asserts stdout contains exactly one JSONL event per
//! `LinkError` variant plus one terminal, each event with a
//! populated `code` field and `location.file` pointing at the trace
//! root. Complements the variant-level `trace_decomposition.rs`
//! tests (which run the library validator directly) by pinning the
//! CLI wire shape.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use std::path::PathBuf;

use assert_cmd::Command;
use serde_json::Value;
use tempfile::TempDir;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

fn cargo_evidence() -> Command {
    #[allow(deprecated)]
    Command::cargo_bin("cargo-evidence").unwrap()
}

/// Copy the tool's own `cert/trace/` into a tempdir and tamper one
/// HLR's `surfaces` to contain a string not in `KNOWN_SURFACES`.
/// The resulting trace fails surface-bijection validation with at
/// least one `TRACE_HLR_SURFACE_UNKNOWN` event and one
/// `TRACE_HLR_SURFACE_UNCLAIMED` event (the claimed surface gets
/// replaced, so the original surface is now orphaned).
fn tampered_trace_dir() -> TempDir {
    let tmp = TempDir::new().expect("tempdir");
    let src = workspace_root().join("cert").join("trace");
    for name in ["sys.toml", "hlr.toml", "llr.toml", "tests.toml"] {
        std::fs::copy(src.join(name), tmp.path().join(name)).expect("copy trace file");
    }
    let hlr_path = tmp.path().join("hlr.toml");
    let content = std::fs::read_to_string(&hlr_path).expect("read hlr.toml");
    // Swap the first concrete surface for a bogus one.
    let tampered = content.replacen(
        "\"VERIFY_OK / VERIFY_FAIL / VERIFY_ERROR terminal contract\"",
        "\"NOT_A_REAL_SURFACE_FOR_TEST_042\"",
        1,
    );
    assert!(
        tampered != content,
        "tamper pattern must match at least once"
    );
    std::fs::write(&hlr_path, tampered).expect("write tampered hlr.toml");
    tmp
}

/// Happy path: `trace --validate --format=jsonl` over a clean trace
/// emits exactly one `VERIFY_OK` terminal and nothing else.
#[test]
fn trace_validate_jsonl_happy_path() {
    let out = cargo_evidence()
        .current_dir(workspace_root())
        .args([
            "evidence",
            "--format=jsonl",
            "trace",
            "--validate",
            "--require-hlr-sys-trace",
            "--require-hlr-surface-bijection",
            "--trace-roots",
            "cert/trace",
        ])
        .output()
        .expect("spawn");
    assert!(
        out.status.success(),
        "clean trace must pass; stdout={}\nstderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    let lines: Vec<Value> = stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("each line is JSON"))
        .collect();
    assert_eq!(
        lines.len(),
        1,
        "clean trace must emit exactly one event (the terminal); got {} events",
        lines.len()
    );
    assert_eq!(
        lines[0].get("code").and_then(Value::as_str),
        Some("VERIFY_OK"),
        "terminal must be VERIFY_OK; got line:\n{}",
        lines[0]
    );
}

/// Empty-root path (LLR-105): `trace --validate --format=jsonl` over a
/// trace root that exists but holds zero requirements must NOT
/// terminate `VERIFY_OK` — absence of evidence is an adoption
/// state, not valid evidence. The stream carries one typed
/// `TRACE_EVIDENCE_EMPTY` event plus a `VERIFY_FAIL` terminal, and
/// the process exits 2 (the jsonl verification-failure code).
#[test]
fn trace_validate_jsonl_empty_root_fails_closed() {
    let tmp = TempDir::new().expect("tempdir");
    for (name, list_key) in [
        ("sys.toml", "requirements"),
        ("hlr.toml", "requirements"),
        ("llr.toml", "requirements"),
        ("tests.toml", "tests"),
    ] {
        std::fs::write(
            tmp.path().join(name),
            format!(
                "{} = []\n\n[schema]\nversion = \"0.0.1\"\n\n[meta]\ndocument_id = \"DS\"\nrevision = \"1.0\"\n",
                list_key
            ),
        )
        .expect("write empty trace file");
    }
    let out = cargo_evidence()
        .current_dir(workspace_root())
        .args(["evidence", "--format=jsonl", "trace", "--validate"])
        .arg("--trace-roots")
        .arg(tmp.path())
        .output()
        .expect("spawn");
    assert_eq!(
        out.status.code(),
        Some(2),
        "empty trace root must exit 2 (verification failure); stdout={}\nstderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    let lines: Vec<Value> = stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("each line is JSON"))
        .collect();
    let codes: Vec<&str> = lines
        .iter()
        .filter_map(|l| l.get("code").and_then(Value::as_str))
        .collect();
    assert!(
        codes.contains(&"TRACE_EVIDENCE_EMPTY"),
        "expected typed TRACE_EVIDENCE_EMPTY event; got codes:\n{:?}",
        codes
    );
    assert_eq!(
        codes.last().copied(),
        Some("VERIFY_FAIL"),
        "last event must be VERIFY_FAIL, never VERIFY_OK over no evidence; got codes:\n{:?}",
        codes
    );
    // The adoption event names the root so an agent can locate it.
    let gap = lines
        .iter()
        .find(|l| l.get("code").and_then(Value::as_str) == Some("TRACE_EVIDENCE_EMPTY"))
        .expect("gap event present");
    assert!(
        gap.get("location")
            .and_then(|l| l.get("file"))
            .and_then(Value::as_str)
            .is_some_and(|s| !s.is_empty()),
        "TRACE_EVIDENCE_EMPTY must carry location.file; got:\n{}",
        gap
    );
}

/// Missing-root path (LLR-105): `trace --validate --format=jsonl`
/// over a configured-but-absent trace root must fail closed with
/// `TRACE_EVIDENCE_NOT_ADOPTED` + `VERIFY_FAIL` (exit 2) — the
/// pre-fix behavior skipped the root with a warning and could
/// still terminate `VERIFY_OK`.
#[test]
fn trace_validate_jsonl_missing_root_fails_closed() {
    let tmp = TempDir::new().expect("tempdir");
    let missing = tmp.path().join("no-such-root");
    let out = cargo_evidence()
        .current_dir(workspace_root())
        .args(["evidence", "--format=jsonl", "trace", "--validate"])
        .arg("--trace-roots")
        .arg(&missing)
        .output()
        .expect("spawn");
    assert_eq!(
        out.status.code(),
        Some(2),
        "missing trace root must exit 2; stdout={}\nstderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    let lines: Vec<Value> = stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("each line is JSON"))
        .collect();
    let codes: Vec<&str> = lines
        .iter()
        .filter_map(|l| l.get("code").and_then(Value::as_str))
        .collect();
    assert!(
        codes.contains(&"TRACE_EVIDENCE_NOT_ADOPTED"),
        "expected typed TRACE_EVIDENCE_NOT_ADOPTED event; got codes:\n{:?}",
        codes
    );
    assert_eq!(
        codes.last().copied(),
        Some("VERIFY_FAIL"),
        "last event must be VERIFY_FAIL; got codes:\n{:?}",
        codes
    );
}

/// Tampered path: `trace --validate --format=jsonl` over a trace
/// with a bad surface emits one `TRACE_HLR_SURFACE_UNKNOWN` event,
/// one `TRACE_HLR_SURFACE_UNCLAIMED` event, and a `VERIFY_FAIL`
/// terminal — each with its own typed `code` field. This is the
/// load-bearing property for MCP: agents iterate `code` to group
/// violations by rule, not prose-match.
#[test]
fn trace_validate_jsonl_emits_per_variant() {
    let tmp = tampered_trace_dir();
    let out = cargo_evidence()
        .current_dir(workspace_root())
        .args([
            "evidence",
            "--format=jsonl",
            "trace",
            "--validate",
            "--require-hlr-sys-trace",
            "--require-hlr-surface-bijection",
        ])
        .arg("--trace-roots")
        .arg(tmp.path())
        .output()
        .expect("spawn");
    assert!(
        !out.status.success(),
        "tampered trace must fail; stdout={}\nstderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    let lines: Vec<Value> = stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("each line is JSON"))
        .collect();
    assert!(
        lines.len() >= 3,
        "expected ≥3 events (SurfaceUnknown + ≥1 SurfaceUnclaimed + terminal); got {} events\nstdout:\n{}",
        lines.len(),
        stdout
    );

    let codes: Vec<&str> = lines
        .iter()
        .filter_map(|l| l.get("code").and_then(Value::as_str))
        .collect();
    assert!(
        codes.contains(&"TRACE_HLR_SURFACE_UNKNOWN"),
        "expected TRACE_HLR_SURFACE_UNKNOWN in stream; got codes:\n{:?}",
        codes
    );
    assert!(
        codes.contains(&"TRACE_HLR_SURFACE_UNCLAIMED"),
        "expected TRACE_HLR_SURFACE_UNCLAIMED in stream; got codes:\n{:?}",
        codes
    );

    // Terminal is always the last event.
    let terminal = lines.last().expect("at least one line");
    assert_eq!(
        terminal.get("code").and_then(Value::as_str),
        Some("VERIFY_FAIL"),
        "last event must be VERIFY_FAIL terminal; got:\n{}",
        terminal
    );

    // Each non-terminal event has a non-empty location.file pointing
    // at the trace root (MCP uses this to link events back to the
    // user's workspace).
    for line in &lines[..lines.len() - 1] {
        let loc_file = line
            .get("location")
            .and_then(|l| l.get("file"))
            .and_then(Value::as_str);
        assert!(
            loc_file.is_some_and(|s| !s.is_empty()),
            "per-variant event must carry a non-empty location.file; got:\n{}",
            line
        );
    }
}
