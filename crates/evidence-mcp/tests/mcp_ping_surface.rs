//! Surface tests for `evidence_ping` (TEST-066).
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

use helpers::{init_frames, session};

/// TEST-066 selector: `evidence_ping` on the self-repo returns
/// `skew == "matched"` (the locally-built binaries on `PATH` come
/// from the same workspace tree as evidence-mcp), with
/// `mcp_version` and `cli_version` both populated and equal. Pins
/// the happy-path shape of [`evidence_mcp::schema::PingResponse`].
#[test]
fn evidence_ping_matched_returns_versions_and_matched_skew() {
    let mut frames = init_frames();
    frames.push(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {"name": "evidence_ping", "arguments": {}}
    }));

    let responses = session(&frames, 2);
    assert_eq!(
        responses.len(),
        2,
        "expected init + ping responses; got: {responses:?}"
    );

    let call_resp = &responses[1];

    // Happy path rides on structuredContent, not a JSON-RPC
    // error or rmcp's isError flag. Match the failure-surface
    // convention so a regression that starts routing ping
    // through the error path fires loud.
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
        "happy-path ping must not set isError==true; got {call_resp}"
    );

    let structured = call_resp
        .pointer("/result/structuredContent")
        .unwrap_or_else(|| panic!("missing structuredContent: {call_resp}"));

    let mcp_version = structured["mcp_version"]
        .as_str()
        .unwrap_or_else(|| panic!("missing mcp_version: {structured}"));
    assert!(
        !mcp_version.is_empty() && mcp_version.chars().any(|c| c.is_ascii_digit()),
        "mcp_version must be non-empty and contain a digit; got {mcp_version:?}"
    );

    // Skew state: locally-built cargo-evidence and evidence-mcp
    // share a workspace, so the probe should match. Skew tag is
    // a short string — pin the value, not just the presence.
    assert_eq!(
        structured["skew"].as_str(),
        Some("matched"),
        "expected skew == 'matched' on self-repo; structured={structured}"
    );

    // On matched, cli_version equals mcp_version and there is
    // no probe_error.
    assert_eq!(
        structured["cli_version"].as_str(),
        Some(mcp_version),
        "cli_version must equal mcp_version on matched; structured={structured}"
    );
    assert!(
        structured.get("probe_error").is_none() || structured["probe_error"].is_null(),
        "probe_error must be absent on matched; structured={structured}"
    );
}

/// TEST-066 selector: a typo'd argument field on `evidence_ping`
/// (which takes no arguments) must fail at the MCP layer rather
/// than silently succeed. Pins the
/// `#[serde(deny_unknown_fields)]` contract on `PingRequest`
/// that matches the other MCP verbs (HLR-054 / LLR-054).
#[test]
fn evidence_ping_rejects_unknown_field_typo() {
    let mut frames = init_frames();
    frames.push(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": "evidence_ping",
            "arguments": {"unexpected": "nope"}
        }
    }));

    let responses = session(&frames, 2);
    assert_eq!(responses.len(), 2, "responses: {responses:?}");
    let call_resp = &responses[1];
    let is_error = call_resp.get("error").is_some();
    let is_error_flag = call_resp
        .pointer("/result/isError")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    assert!(
        is_error || is_error_flag,
        "expected either a JSON-RPC error or isError:true on a typo'd field; got: {call_resp}"
    );
}
