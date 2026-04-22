//! End-to-end MCP surface tests (TEST-050).
//!
//! Each test spawns the release-built `evidence-mcp` binary via
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

/// Spawn the built `evidence-mcp` binary with piped stdio.
///
/// The MCP wrapper internally calls `cargo evidence <verb>`,
/// which cargo resolves via `$PATH` to the `cargo-evidence`
/// binary. During `cargo test`, we want that spawn to pick up
/// the *locally-built* `cargo-evidence` (in `target/<profile>/`)
/// rather than whatever version the developer has installed via
/// `cargo install` — the installed copy may be stale and is
/// irrelevant to this test. `assert_cmd::cargo::cargo_bin`
/// returns paths under `target/<profile>/`; its parent is the
/// right dir to prepend to `PATH`.
fn spawn_server() -> Child {
    let bin = assert_cmd::cargo::cargo_bin("evidence-mcp");
    assert!(
        bin.exists(),
        "evidence-mcp binary missing at {bin:?} — run `cargo build -p evidence-mcp` first"
    );
    let target_dir = bin
        .parent()
        .expect("evidence-mcp binary has a parent dir")
        .to_path_buf();
    // Construct the new PATH with platform-correct separator:
    // `:` on Unix, `;` on Windows. `std::env::join_paths` handles
    // that and rejects entries containing the separator (e.g. a
    // weirdly-named directory), which is the right failure mode
    // here — a malformed PATH is worth failing loud, not silent.
    let mut entries: Vec<std::path::PathBuf> = vec![target_dir];
    if let Some(existing) = std::env::var_os("PATH") {
        entries.extend(std::env::split_paths(&existing));
    }
    let new_path = std::env::join_paths(entries).expect("valid PATH entries");
    Command::new(&bin)
        .env("PATH", new_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn evidence-mcp")
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

    child.wait().ok();
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
    assert!(
        !is_error,
        "tool call unexpectedly flagged isError: {call_resp}"
    );
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

/// TEST-050 selector: an evidence_check on an empty directory
/// must return a well-formed structured MCP response (not a
/// transport error) with the expected failure terminal. The CLI
/// emits `CLI_INVALID_ARGUMENT` + `VERIFY_FAIL` in this
/// condition; the MCP wrapper should surface both cleanly.
#[test]
fn evidence_check_source_on_empty_dir_fails_gracefully() {
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

    let responses = session(&frames, 2);
    assert_eq!(responses.len(), 2, "responses: {responses:?}");

    let call_resp = &responses[1];
    let structured = call_resp
        .pointer("/result/structuredContent")
        .unwrap_or_else(|| panic!("missing structuredContent: {call_resp}"));

    // Terminal should be a failure terminal. The CLI happens to
    // emit VERIFY_FAIL for an empty dir (with CLI_INVALID_ARGUMENT
    // as the first diagnostic), but we pin the *shape* not the
    // exact failure wording so the assertion survives CLI-side
    // wording tweaks.
    let terminal = structured["terminal"].as_str().unwrap_or("");
    assert!(
        terminal.ends_with("_FAIL") || terminal.ends_with("_ERROR"),
        "expected a failure terminal (*_FAIL or *_ERROR); got {terminal:?}; structured={structured}"
    );
    let exit = structured["exit_code"].as_i64().unwrap_or(0);
    assert_ne!(
        exit, 0,
        "failure run must not exit 0; structured={structured}"
    );
    // Response shape must still be well-formed — structuredContent
    // present, diagnostics array, no rmcp-layer error flag.
    let is_error = call_resp
        .pointer("/result/isError")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    assert!(
        !is_error,
        "MCP result should NOT have isError==true on a failing subcommand; the failure \
         is encoded in the structured response. Got: {call_resp}"
    );
}

/// TEST-050 selector (opt-in): a full `evidence_check` source-mode
/// run on the self-repo passes with `VERIFY_OK`. Expensive —
/// spawns `cargo test --workspace` under the MCP process which
/// is itself running under `cargo test`. Historically causes
/// `target/` lock contention (see
/// `crates/cargo-evidence/src/cli/check.rs:191-196`). Gated
/// behind `MCP_RUN_LONG_CHECK=1` so normal CI skips it; run
/// manually with
/// `MCP_RUN_LONG_CHECK=1 cargo test -p evidence-mcp -- --ignored`.
#[test]
#[ignore = "nested cargo-test; opt-in via MCP_RUN_LONG_CHECK=1"]
fn evidence_check_source_on_self_repo_is_opt_in() {
    if std::env::var_os("MCP_RUN_LONG_CHECK").is_none() {
        return;
    }

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
            "name": "evidence_check",
            "arguments": {
                "workspace_path": workspace_root.to_str().expect("utf-8 path"),
                "mode": "source"
            }
        }
    }));

    let responses = session(&frames, 2);
    let call_resp = &responses[1];
    let structured = call_resp
        .pointer("/result/structuredContent")
        .unwrap_or_else(|| panic!("missing structuredContent: {call_resp}"));

    assert_eq!(
        structured["terminal"].as_str(),
        Some("VERIFY_OK"),
        "expected VERIFY_OK terminal; structured={structured}"
    );
    assert_eq!(structured["exit_code"].as_i64(), Some(0));
}
