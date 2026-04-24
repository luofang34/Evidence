//! Surface tests for `evidence_floors` (TEST-067).
//!
//! Separate integration-test binary from `mcp_surface.rs` so the
//! parent stays under the workspace 500-line limit. Shares the
//! `helpers` module via `#[path]`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use serde_json::{Value, json};

#[path = "mcp_surface/helpers.rs"]
mod helpers;

use helpers::{init_frames, session, session_in};

/// TEST-067 selector: `evidence_floors` on the self-repo reads
/// `cert/floors.toml`, measures each dimension, and terminates
/// with `FLOORS_OK` (the self-repo's ratchet gate is green). Pins
/// the happy-path shape of the JSONL pass-through — `terminal`,
/// `exit_code`, per-dimension `FLOORS_DIMENSION_OK` entries.
#[test]
fn evidence_floors_on_self_repo_terminates_with_floors_ok() {
    let workspace_root = std::env::var("CARGO_MANIFEST_DIR")
        .map(std::path::PathBuf::from)
        .expect("CARGO_MANIFEST_DIR")
        .parent()
        .expect("crates/")
        .parent()
        .expect("workspace root")
        .to_path_buf();

    let mut frames = init_frames();
    frames.push(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": "evidence_floors",
            "arguments": {"workspace_path": workspace_root.to_str().expect("utf-8 path")}
        }
    }));

    let responses = session(&frames, 2);
    assert_eq!(responses.len(), 2, "responses: {responses:?}");

    let call_resp = &responses[1];
    let structured = call_resp
        .pointer("/result/structuredContent")
        .unwrap_or_else(|| panic!("missing structuredContent: {call_resp}"));

    assert_eq!(
        structured["terminal"].as_str(),
        Some("FLOORS_OK"),
        "expected FLOORS_OK terminal on self-repo; structured={structured}"
    );
    assert_eq!(
        structured["exit_code"].as_i64(),
        Some(0),
        "expected exit_code == 0; structured={structured}"
    );

    let diagnostics = structured["diagnostics"]
        .as_array()
        .unwrap_or_else(|| panic!("diagnostics not array: {structured}"));
    let per_dim_count = diagnostics
        .iter()
        .filter(|d| d.get("code").and_then(Value::as_str) == Some("FLOORS_DIMENSION_OK"))
        .count();
    assert!(
        per_dim_count > 0,
        "expected at least one FLOORS_DIMENSION_OK diagnostic; got: {diagnostics:?}"
    );
}

/// TEST-067 selector: omitting `workspace_path` prepends the
/// synthetic `MCP_WORKSPACE_FALLBACK` warning (LLR-054 chain).
/// Spawns the server with an empty tempdir as CWD so the fallback
/// path lands somewhere that fails fast (no `cert/floors.toml`)
/// instead of firing on the Evidence workspace.
#[test]
fn evidence_floors_missing_workspace_path_emits_fallback_signal() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut frames = init_frames();
    frames.push(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {"name": "evidence_floors", "arguments": {}}
    }));

    let responses = session_in(&frames, 2, Some(tmp.path()));
    assert_eq!(responses.len(), 2, "responses: {responses:?}");
    let call_resp = &responses[1];
    let structured = call_resp
        .pointer("/result/structuredContent")
        .unwrap_or_else(|| panic!("missing structuredContent: {call_resp}"));
    let diagnostics = structured["diagnostics"]
        .as_array()
        .unwrap_or_else(|| panic!("diagnostics not array: {structured}"));

    let first = diagnostics
        .first()
        .unwrap_or_else(|| panic!("diagnostics was empty: {structured}"));
    assert_eq!(
        first["code"].as_str(),
        Some("MCP_WORKSPACE_FALLBACK"),
        "first diagnostic must be the fallback signal; got {first}"
    );

    let summary = structured["summary"]
        .as_object()
        .unwrap_or_else(|| panic!("summary not object: {structured}"));
    assert!(
        summary
            .get("MCP_WORKSPACE_FALLBACK")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            >= 1,
        "summary must track the fallback signal: {summary:?}"
    );

    // Terminal: a tempdir with no `cert/floors.toml` hits the
    // CLI's missing-config path, which emits FLOORS_OK as a
    // friendly "gate not configured" info. Pin the terminal so
    // a regression that drops it (or flips it to empty) fires
    // here, not at the next reviewer's eyeball.
    assert_eq!(
        structured["terminal"].as_str(),
        Some("FLOORS_OK"),
        "expected FLOORS_OK on fallback-to-tempdir (no floors config); structured={structured}"
    );
}

/// TEST-070 selector: `evidence_floors` on a workspace whose
/// `cert/floors.toml` declares an unsatisfiable floor surfaces
/// `FLOORS_FAIL` + `FLOORS_BELOW_MIN`. Exercises the failure-
/// path shape symmetric to `evidence_floors_on_self_repo_
/// terminates_with_floors_ok`.
#[test]
fn evidence_floors_below_floor_terminates_with_floors_fail() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let cert = tmp.path().join("cert");
    std::fs::create_dir(&cert).expect("create cert dir");
    // `diagnostic_codes` measures `evidence_core::RULES.len()` —
    // a compiled-in constant — so the measurement is available
    // regardless of workspace contents. Floor = 999_999 is
    // unreachable; current will be whatever the crate ships.
    std::fs::write(
        cert.join("floors.toml"),
        "schema_version = 1\n\n[floors]\ndiagnostic_codes = 999999\n",
    )
    .expect("write floors.toml");

    let mut frames = init_frames();
    frames.push(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": "evidence_floors",
            "arguments": {"workspace_path": tmp.path().to_str().expect("utf-8 path")}
        }
    }));

    let responses = session(&frames, 2);
    assert_eq!(responses.len(), 2, "responses: {responses:?}");

    let call_resp = &responses[1];
    let structured = call_resp
        .pointer("/result/structuredContent")
        .unwrap_or_else(|| panic!("missing structuredContent: {call_resp}"));

    assert_eq!(
        structured["terminal"].as_str(),
        Some("FLOORS_FAIL"),
        "expected FLOORS_FAIL terminal; structured={structured}"
    );
    assert_eq!(
        structured["exit_code"].as_i64(),
        Some(2),
        "expected exit_code == 2 on FLOORS_FAIL; structured={structured}"
    );

    let diagnostics = structured["diagnostics"]
        .as_array()
        .unwrap_or_else(|| panic!("diagnostics not array: {structured}"));
    // The FLOORS_BELOW_MIN diagnostic must name `diagnostic_codes`
    // specifically — a regression that flips which dimension
    // gets flagged (or emits the code without a dimension at
    // all) would pass a looser "any FLOORS_BELOW_MIN present"
    // check. The CLI today encodes the dimension as the
    // message prefix (`"{dimension} ({kind}): current=…"`);
    // assert the prefix rather than a dedicated field since
    // Diagnostic carries no structured `dimension` slot yet.
    let below_min_for_dim = diagnostics.iter().find(|d| {
        d.get("code").and_then(Value::as_str) == Some("FLOORS_BELOW_MIN")
            && d.get("message")
                .and_then(Value::as_str)
                .is_some_and(|m| m.starts_with("diagnostic_codes "))
    });
    assert!(
        below_min_for_dim.is_some(),
        "expected a FLOORS_BELOW_MIN diagnostic naming `diagnostic_codes`; got: {diagnostics:?}"
    );
}

/// TEST-067 selector: a typo'd argument field on
/// `evidence_floors` (e.g., `"workspace"` instead of
/// `"workspace_path"`) must fail at the MCP layer, not silently
/// fall through to the server's CWD. Pins the
/// `#[serde(deny_unknown_fields)]` contract on `FloorsRequest`.
#[test]
fn evidence_floors_rejects_unknown_field_typo() {
    let mut frames = init_frames();
    frames.push(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": "evidence_floors",
            "arguments": {"workspace": "/tmp/wrong-dir"}
        }
    }));

    let responses = session(&frames, 2);
    assert_eq!(responses.len(), 2, "responses: {responses:?}");
    let call_resp = &responses[1];
    let is_error = call_resp.get("error").is_some();
    let terminal_is_error = call_resp
        .pointer("/result/structuredContent/terminal")
        .and_then(Value::as_str)
        .is_some_and(|t| t.ends_with("_ERROR") || t.ends_with("_FAIL"));
    let is_error_flag = call_resp
        .pointer("/result/isError")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    assert!(
        is_error || terminal_is_error || is_error_flag,
        "expected an error signal on a typo'd field; got: {call_resp}"
    );
}
