//! Tool-layer failure-mode surface tests (TEST-063).
//!
//! Covers the contract from LLR-063: every `RunError` variant
//! plus the synthesized parse terminals surface as a well-formed
//! `JsonlToolResponse` (or `RulesToolResponse`) carrying the
//! matching `MCP_*` code, not as an rmcp `Err(String)`.
//!
//! Separate integration-test binary (rather than living alongside
//! the happy-path tests in `mcp_surface.rs`) so the `mcp_surface`
//! file stays under the workspace 500-line limit.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use serde_json::{Value, json};

#[path = "mcp_surface/helpers.rs"]
mod helpers;

use helpers::{Path, init_frames, session_in_with_path};

/// TEST-063 selector: a subprocess failure (here `cargo` not on
/// PATH, simulated by setting `PATH` to `target/<profile>/` only)
/// surfaces as a well-formed `JsonlToolResponse` carrying an
/// `MCP_CARGO_NOT_FOUND` diagnostic + `exit_code == 2`, not as
/// an rmcp `Err(String)`.
#[test]
fn evidence_check_cargo_not_found_returns_structured_diagnostic() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut frames = init_frames();
    frames.push(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": "evidence_check",
            "arguments": {
                "workspace_path": tmp.path().to_str().expect("utf-8 path"),
                "mode": "auto"
            }
        }
    }));

    let responses = session_in_with_path(&frames, 2, None, Path::TargetDirOnly);
    assert_eq!(responses.len(), 2, "responses: {responses:?}");

    let call_resp = &responses[1];
    let structured = call_resp
        .pointer("/result/structuredContent")
        .unwrap_or_else(|| panic!("missing structuredContent: {call_resp}"));

    // Failure rides on the structured response body, not on a
    // JSON-RPC transport error or rmcp's isError flag.
    assert!(
        call_resp.get("error").is_none(),
        "expected no JSON-RPC error object; got {call_resp}"
    );
    let is_error = call_resp
        .pointer("/result/isError")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    assert!(
        !is_error,
        "tool-layer failure must ride on structuredContent, not isError: {call_resp}"
    );

    assert_eq!(
        structured["exit_code"].as_i64(),
        Some(2),
        "tool-layer failure should carry exit_code == 2; structured={structured}"
    );
    assert_eq!(
        structured["terminal"].as_str(),
        Some("MCP_CARGO_NOT_FOUND"),
        "expected MCP_CARGO_NOT_FOUND terminal; structured={structured}"
    );

    let diagnostics = structured["diagnostics"]
        .as_array()
        .unwrap_or_else(|| panic!("diagnostics not array: {structured}"));
    let carries_code = diagnostics
        .iter()
        .any(|d| d.get("code").and_then(Value::as_str) == Some("MCP_CARGO_NOT_FOUND"));
    assert!(
        carries_code,
        "diagnostics array must contain an MCP_CARGO_NOT_FOUND entry: {diagnostics:?}"
    );

    let summary = structured["summary"]
        .as_object()
        .unwrap_or_else(|| panic!("summary not object: {structured}"));
    assert!(
        summary
            .get("MCP_CARGO_NOT_FOUND")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            >= 1,
        "summary must track MCP_CARGO_NOT_FOUND: {summary:?}"
    );
}
