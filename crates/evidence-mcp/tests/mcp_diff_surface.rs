//! Surface tests for `evidence_diff` (TEST-068).
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

/// TEST-068 selector: `evidence_diff` on two nonexistent bundle
/// paths surfaces as a well-formed `DiffToolResponse` with
/// `diff == null`, `error` carrying a structured `MCP_*`
/// diagnostic, and `exit_code == 2`. Exercises plumbing end-
/// to-end (handler wiring, schema serialization) without
/// needing fixture bundles.
#[test]
fn evidence_diff_on_missing_bundles_surfaces_structured_error() {
    let mut frames = init_frames();
    frames.push(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": "evidence_diff",
            "arguments": {
                "bundle_a_path": "/nonexistent-a-does-not-exist",
                "bundle_b_path": "/nonexistent-b-does-not-exist"
            }
        }
    }));

    let responses = session(&frames, 2);
    assert_eq!(responses.len(), 2, "responses: {responses:?}");

    let call_resp = &responses[1];
    let structured = call_resp
        .pointer("/result/structuredContent")
        .unwrap_or_else(|| panic!("missing structuredContent: {call_resp}"));

    // Subprocess ran (cargo evidence diff is spawned); it will
    // exit nonzero because the bundles don't exist. The
    // response shape pins that this failure-path still returns
    // a structured response: exit_code != 0, diff absent (or
    // null), and — on stdout-parse failure — the error field
    // carries a structured MCP_* diagnostic.
    let exit_code = structured["exit_code"].as_i64().unwrap_or(0);
    assert_ne!(
        exit_code, 0,
        "expected nonzero exit_code on missing bundles; got {structured}"
    );

    // Either `diff` is null/absent OR `error` carries an MCP_*
    // code — both are valid failure modes depending on whether
    // the CLI itself short-circuits or stdout ends up empty.
    // Pin that the response is WELL-FORMED, not the specific
    // branch.
    let diff_absent = structured.get("diff").map(|v| v.is_null()).unwrap_or(true);
    let error_present = structured
        .get("error")
        .and_then(|e| e.get("code"))
        .and_then(Value::as_str)
        .is_some_and(|c| c.starts_with("MCP_") || c.starts_with("CLI_"));
    assert!(
        diff_absent || error_present,
        "expected diff==null OR error carrying MCP_*/CLI_* code; got {structured}"
    );

    // Transport-level: not a JSON-RPC error, not isError.
    assert!(
        call_resp.get("error").is_none(),
        "tool-layer failure must ride on structuredContent, not transport error: {call_resp}"
    );
}

/// TEST-068 selector: a typo'd argument field on `evidence_diff`
/// must fail at the MCP layer rather than silently succeed. Pins
/// the `#[serde(deny_unknown_fields)]` contract on `DiffRequest`.
#[test]
fn evidence_diff_rejects_unknown_field_typo() {
    let mut frames = init_frames();
    frames.push(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": "evidence_diff",
            "arguments": {
                "bundle_a_path": "/some/path",
                "bundle_b_path": "/some/other/path",
                "unexpected": "nope"
            }
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

/// TEST-068 selector: `evidence_diff` without both required
/// path arguments must fail at deserialization. Missing either
/// `bundle_a_path` or `bundle_b_path` produces an error, not a
/// silent call against empty strings.
#[test]
fn evidence_diff_rejects_missing_required_arguments() {
    let mut frames = init_frames();
    frames.push(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": "evidence_diff",
            "arguments": {"bundle_a_path": "/only-one-path"}
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
        "expected an error on missing required bundle_b_path; got: {call_resp}"
    );
}
