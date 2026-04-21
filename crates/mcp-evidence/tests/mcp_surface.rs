//! End-to-end MCP surface tests (TEST-050).
//!
//! Each test spawns the release-built `mcp-evidence` binary via
//! `assert_cmd`, drives it with a scripted MCP JSON-RPC conversation
//! over stdio, and asserts on the structured tool response. This
//! exercises the full `rmcp` stack — init handshake, `tools/list`
//! routing, `Parameters<T>` deserialization, `Json<T>` response
//! serialization — against the exact binary a Claude host would
//! register.
//!
//! Commits 3+ add one tool method per commit; this file grows
//! correspondingly.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{Value, json};

/// Spawn the release binary with piped stdio. Release (not debug)
/// is the target for smoke tests that also validate the published
/// artifact shape.
fn spawn_server() -> Child {
    let bin = assert_cmd::cargo::cargo_bin("mcp-evidence");
    assert!(
        bin.exists(),
        "mcp-evidence binary missing at {bin:?} — run `cargo build -p mcp-evidence` first"
    );
    Command::new(bin)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn mcp-evidence")
}

/// Drive a scripted MCP session. Writes each frame on its own
/// line (the stdio transport expects newline-delimited JSON),
/// then reads back `expect_responses` response lines and returns
/// them parsed.
fn session(frames: &[Value], expect_responses: usize) -> Vec<Value> {
    let mut child = spawn_server();
    let mut stdin: ChildStdin = child.stdin.take().expect("stdin");
    let stdout: ChildStdout = child.stdout.take().expect("stdout");
    let mut reader = BufReader::new(stdout);

    for frame in frames {
        writeln!(stdin, "{}", serde_json::to_string(frame).expect("encode")).expect("write");
    }
    drop(stdin);

    let mut responses = Vec::with_capacity(expect_responses);
    for _ in 0..expect_responses {
        let mut line = String::new();
        let n = reader.read_line(&mut line).expect("read_line");
        if n == 0 {
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        responses.push(serde_json::from_str::<Value>(trimmed).expect("parse response"));
    }

    let _ = child.wait();
    responses
}

fn init_frames() -> Vec<Value> {
    vec![
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "mcp-surface-test", "version": "0"}
            }
        }),
        json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
            "params": {}
        }),
    ]
}

/// TEST-050 selector: every tool call flows through rmcp's
/// `Json<RulesToolResponse>` return path; the `count` field
/// equals the library's `evidence_core::RULES.len()`. Catches
/// any future drift between CLI wire shape and the library
/// source-of-truth const via the full MCP surface.
#[test]
fn evidence_rules_count_matches_library_const() {
    let mut frames = init_frames();
    frames.push(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {"name": "evidence_rules", "arguments": {}}
    }));

    let responses = session(&frames, 2);
    assert_eq!(
        responses.len(),
        2,
        "expected init response + tools/call response; got: {responses:?}"
    );

    let call_resp = &responses[1];
    let structured = call_resp
        .pointer("/result/structuredContent")
        .unwrap_or_else(|| panic!("missing structuredContent: {call_resp}"));

    let count = structured["count"]
        .as_u64()
        .unwrap_or_else(|| panic!("count not u64: {structured}"));
    assert_eq!(
        count as usize,
        evidence_core::RULES.len(),
        "MCP-reported rules.count drift from library const: mcp={count} lib={}",
        evidence_core::RULES.len()
    );
}

/// TEST-050 selector: `exit_code == 0` on success. Pins the
/// exit-code wiring so future rmcp-result-conversion changes
/// don't silently drop the signal.
#[test]
fn evidence_rules_exit_code_zero_on_success() {
    let mut frames = init_frames();
    frames.push(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {"name": "evidence_rules", "arguments": {}}
    }));

    let responses = session(&frames, 2);
    let call_resp = &responses[1];
    let structured = call_resp
        .pointer("/result/structuredContent")
        .unwrap_or_else(|| panic!("missing structuredContent: {call_resp}"));

    assert_eq!(
        structured["exit_code"].as_i64(),
        Some(0),
        "expected exit_code == 0; got {structured}"
    );
    // isError flag should not be set for a successful call.
    let is_error = call_resp
        .pointer("/result/isError")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    assert!(!is_error, "tool call unexpectedly flagged isError: {call_resp}");
}

/// TEST-050 selector: the MCP wrapper correctly pipes the CLI's
/// doctor rigor audit through — the workspace_path argument is
/// honored, the JSONL stream is parsed into a structured
/// response, and the terminal code reaches the agent. Mirrors
/// the CLI's `doctor_cmd::current_workspace_passes_doctor`
/// through the MCP layer.
///
/// Expensive: the doctor subprocess actually walks the workspace.
/// Fast enough (<2s on dev hardware) to keep non-opt-in.
#[test]
fn evidence_doctor_on_self_repo_passes() {
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
            "name": "evidence_doctor",
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
        Some("DOCTOR_OK"),
        "expected DOCTOR_OK terminal; structured={structured}"
    );
    assert_eq!(
        structured["exit_code"].as_i64(),
        Some(0),
        "expected exit_code == 0; structured={structured}"
    );
    // No error-severity findings — DOCTOR_OK requires it. Warnings
    // are tolerated (e.g. DOCTOR_FLOORS_BOUNDARY_MISMATCH fires
    // whenever a boundary crate lacks a per_crate floor entry —
    // which is transiently the case between `boundary.toml` and
    // `floors.toml` edits).
    let diagnostics = structured["diagnostics"]
        .as_array()
        .unwrap_or_else(|| panic!("diagnostics not array: {structured}"));
    for diag in diagnostics {
        let severity = diag["severity"].as_str().unwrap_or("");
        assert_ne!(
            severity, "error",
            "unexpected error-severity diagnostic on DOCTOR_OK run: {diag}"
        );
    }
}
