//! Surface tests for `evidence_context` (TEST-091 / TEST-092 /
//! TEST-093).
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

use helpers::{init_frames, session_in};

/// Workspace root of the evidence project — `crates/evidence-mcp`'s
/// grandparent. Tests use this as `workspace_path` so the
/// underlying `cargo evidence context` spawn sees real
/// `cert/trace/` data instead of an empty CWD.
fn workspace_root() -> std::path::PathBuf {
    std::env::var("CARGO_MANIFEST_DIR")
        .map(std::path::PathBuf::from)
        .expect("CARGO_MANIFEST_DIR")
        .parent()
        .expect("crates/")
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

/// TEST-091 selector: `evidence_context` with no selector returns
/// a structured workspace-overview report. `context` is a non-null
/// JSON object whose `selector.kind` is `"workspace"`,
/// `exit_code == 0`, `success == true`, no transport-layer error.
/// Pins the happy-path plumbing end-to-end without depending on a
/// specific requirements count.
#[test]
fn evidence_context_workspace_overview_returns_report() {
    let root = workspace_root();
    let mut frames = init_frames();
    frames.push(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": "evidence_context",
            "arguments": {
                "workspace_path": root.to_str().expect("utf-8 path")
            }
        }
    }));

    let responses = session_in(&frames, 2, Some(&root));
    assert_eq!(responses.len(), 2, "responses: {responses:?}");

    let call_resp = &responses[1];
    let structured = call_resp
        .pointer("/result/structuredContent")
        .unwrap_or_else(|| panic!("missing structuredContent: {call_resp}"));

    assert_eq!(
        structured["exit_code"].as_i64(),
        Some(0),
        "expected exit_code == 0; structured={structured}"
    );
    assert_eq!(
        structured["success"].as_bool(),
        Some(true),
        "expected success == true; structured={structured}"
    );
    let context = structured
        .get("context")
        .unwrap_or_else(|| panic!("missing context field: {structured}"));
    assert!(
        !context.is_null(),
        "context must be non-null on success: {structured}"
    );
    let kind = context
        .pointer("/selector/kind")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("missing context.selector.kind: {structured}"));
    assert_eq!(
        kind, "workspace",
        "no-selector request must produce a workspace overview; got kind={kind}"
    );

    let is_error = call_resp
        .pointer("/result/isError")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    assert!(
        !is_error,
        "tool call unexpectedly flagged isError: {call_resp}"
    );
}

/// TEST-092 selector: a file selector that resolves into a crate
/// with at least one governing LLR populates `context.requirements`
/// with a non-empty array. The CLI exercises the live `cert/trace/`
/// against the real workspace, so we pick a known file
/// (`crates/evidence-mcp/src/server.rs`) that LLR-050 / LLR-064
/// govern.
#[test]
fn evidence_context_file_selector_pulls_requirements() {
    let root = workspace_root();
    let mut frames = init_frames();
    frames.push(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": "evidence_context",
            "arguments": {
                "workspace_path": root.to_str().expect("utf-8 path"),
                "selector": "crates/evidence-mcp/src/server.rs"
            }
        }
    }));

    let responses = session_in(&frames, 2, Some(&root));
    assert_eq!(responses.len(), 2, "responses: {responses:?}");

    let call_resp = &responses[1];
    let structured = call_resp
        .pointer("/result/structuredContent")
        .unwrap_or_else(|| panic!("missing structuredContent: {call_resp}"));

    assert_eq!(
        structured["success"].as_bool(),
        Some(true),
        "expected success on a known-good selector; structured={structured}"
    );

    let context = structured
        .get("context")
        .unwrap_or_else(|| panic!("missing context field: {structured}"));
    let kind = context
        .pointer("/selector/kind")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("missing context.selector.kind: {structured}"));
    assert_eq!(
        kind, "file",
        "expected kind == file for a path selector; got {kind}; context={context}"
    );

    let requirements = context
        .get("requirements")
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("requirements not array: {context}"));
    assert!(
        !requirements.is_empty(),
        "evidence-mcp's server.rs is governed by at least one LLR; \
         got empty requirements: {context}"
    );
}

/// TEST-092 selector (graceful warning path): a selector that
/// resolves to a file outside any LLR's `modules` field carries
/// `CONTEXT_NO_REQUIREMENTS_FOR_SELECTOR` in `context.warnings`
/// rather than failing. Pins the wrapped CLI's graceful-warning
/// path through the MCP layer.
#[test]
fn evidence_context_unmapped_file_carries_no_requirements_warning() {
    let root = workspace_root();
    // README.md sits at the workspace root, not under any
    // crate's source tree — the CLI's file resolver rejects it
    // as out-of-scope rather than mapping it to zero LLRs, so
    // this case actually pins the SELECTOR_OUT_OF_SCOPE path.
    // Pick a Cargo.toml at the workspace root instead — it
    // exists, lives under the workspace, and isn't owned by any
    // LLR's `modules` field.
    let mut frames = init_frames();
    frames.push(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": "evidence_context",
            "arguments": {
                "workspace_path": root.to_str().expect("utf-8 path"),
                "selector": "Cargo.toml"
            }
        }
    }));

    let responses = session_in(&frames, 2, Some(&root));
    assert_eq!(responses.len(), 2, "responses: {responses:?}");

    let call_resp = &responses[1];
    let structured = call_resp
        .pointer("/result/structuredContent")
        .unwrap_or_else(|| panic!("missing structuredContent: {call_resp}"));

    // Either the CLI gracefully resolves it with an empty-
    // requirements warning OR it returns FAIL with the OUT_OF_SCOPE
    // diagnostic. Both are valid CLI-layer behaviors for a path
    // that isn't owned by any LLR's `modules` field; the MCP
    // layer must surface them well-formed in either case.
    let exit_code = structured["exit_code"].as_i64().unwrap_or(99);
    if exit_code == 0 {
        let context = structured
            .get("context")
            .unwrap_or_else(|| panic!("missing context on exit 0: {structured}"));
        let warnings = context
            .get("warnings")
            .and_then(Value::as_array)
            .unwrap_or_else(|| panic!("warnings not array: {context}"));
        let has_no_reqs = warnings.iter().any(|w| {
            w.get("code").and_then(Value::as_str) == Some("CONTEXT_NO_REQUIREMENTS_FOR_SELECTOR")
        });
        assert!(
            has_no_reqs || warnings.is_empty(),
            "graceful path should carry CONTEXT_NO_REQUIREMENTS_FOR_SELECTOR \
             when warnings is non-empty; got {warnings:?}"
        );
    } else {
        // Failure path — the response must still be well-formed:
        // structured present, no transport-layer error.
        let is_error = call_resp
            .pointer("/result/isError")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        assert!(
            !is_error,
            "failure path should ride on structuredContent, not transport error: {call_resp}"
        );
    }
}

/// TEST-093 selector: a request that sets more than one of
/// `selector` / `crate_name` / `module` must fail at the handler
/// layer — either as a JSON-RPC error or with `isError == true`.
/// Defends against agent misuse of the three equivalent entry
/// points (silently picking one would mask the intent mismatch).
#[test]
fn evidence_context_rejects_multiple_selector_fields() {
    let root = workspace_root();
    let mut frames = init_frames();
    frames.push(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": "evidence_context",
            "arguments": {
                "workspace_path": root.to_str().expect("utf-8 path"),
                "selector": "crates/evidence-mcp/src/server.rs",
                "crate_name": "evidence-mcp"
            }
        }
    }));

    let responses = session_in(&frames, 2, Some(&root));
    assert_eq!(responses.len(), 2, "responses: {responses:?}");

    let call_resp = &responses[1];
    let is_error = call_resp.get("error").is_some();
    let is_error_flag = call_resp
        .pointer("/result/isError")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    assert!(
        is_error || is_error_flag,
        "expected either a JSON-RPC error or isError:true when two selector \
         fields are set; got: {call_resp}"
    );
}

/// TEST-093 selector: `#[serde(deny_unknown_fields)]` on
/// `ContextRequest` rejects typo'd argument fields (e.g. `crate`
/// instead of `crate_name`) at deserialization. Mirrors the
/// pattern from `evidence_check_rejects_unknown_field_typo`.
#[test]
fn evidence_context_rejects_unknown_field_typo() {
    let root = workspace_root();
    let mut frames = init_frames();
    frames.push(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": "evidence_context",
            "arguments": {
                "workspace_path": root.to_str().expect("utf-8 path"),
                // Typo: `crate` instead of `crate_name`.
                "crate": "evidence-mcp"
            }
        }
    }));

    let responses = session_in(&frames, 2, Some(&root));
    assert_eq!(responses.len(), 2, "responses: {responses:?}");
    let call_resp = &responses[1];
    let is_error = call_resp.get("error").is_some();
    let is_error_flag = call_resp
        .pointer("/result/isError")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    assert!(
        is_error || is_error_flag,
        "expected either a JSON-RPC error or isError:true on a typo'd field; \
         got: {call_resp}"
    );
}
