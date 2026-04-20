//! PR #51 / TEST-042: `cargo evidence trace --validate
//! --format=jsonl` stream-emit contract.
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

/// Copy the tool's own `tool/trace/` into a tempdir and tamper one
/// HLR's `surfaces` to contain a string not in `KNOWN_SURFACES`.
/// The resulting trace fails surface-bijection validation with at
/// least one `TRACE_HLR_SURFACE_UNKNOWN` event and one
/// `TRACE_HLR_SURFACE_UNCLAIMED` event (the claimed surface gets
/// replaced, so the original surface is now orphaned).
fn tampered_trace_dir() -> TempDir {
    let tmp = TempDir::new().expect("tempdir");
    let src = workspace_root().join("tool").join("trace");
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
            "tool/trace",
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
