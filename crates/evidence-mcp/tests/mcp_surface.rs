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

use serde_json::{Value, json};

#[path = "mcp_surface/helpers.rs"]
mod helpers;

use helpers::{init_frames, session, session_in};

/// TEST-069 selector: `tools/list` advertises every `#[tool]`
/// registered on `Server`. Regressions that drop a method from
/// the `ToolRouter` (e.g., a macro-expansion failure that skips
/// one verb) would pass the per-verb surface tests but leave
/// the dropped verb absent from the server's self-description.
#[test]
fn tools_list_advertises_all_six_verbs() {
    const EXPECTED: &[&str] = &[
        "evidence_check",
        "evidence_diff",
        "evidence_doctor",
        "evidence_floors",
        "evidence_ping",
        "evidence_rules",
    ];

    let mut frames = init_frames();
    frames.push(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/list",
        "params": {}
    }));

    let responses = session(&frames, 2);
    assert_eq!(
        responses.len(),
        2,
        "expected init + tools/list responses; got: {responses:?}"
    );

    let call_resp = &responses[1];
    let tools = call_resp
        .pointer("/result/tools")
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("missing result.tools array: {call_resp}"));

    let names: Vec<&str> = tools
        .iter()
        .filter_map(|t| t.get("name").and_then(Value::as_str))
        .collect();

    for expected in EXPECTED {
        assert!(
            names.contains(expected),
            "tools/list missing {expected}; advertised: {names:?}"
        );
    }
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

/// TEST-062 selector: `initialize` response advertises
/// `serverInfo.name == "evidence-mcp"` (not rmcp's default
/// `"rmcp"`). Agents pattern-matching on the server identity
/// need the tool's real name — the default pulled from rmcp's
/// `from_build_env()` makes evidence-mcp indistinguishable
/// from any other rmcp-built server in the handshake. LLR-062
/// pins the override via `#[tool_handler(.., name = "evidence-mcp")]`.
#[test]
fn initialize_server_info_advertises_evidence_mcp_identity() {
    let frames = init_frames();
    let responses = session(&frames, 1);
    assert_eq!(
        responses.len(),
        1,
        "expected single init response; got: {responses:?}"
    );
    let init_resp = &responses[0];
    let name = init_resp
        .pointer("/result/serverInfo/name")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("missing result.serverInfo.name: {init_resp}"));
    assert_eq!(
        name, "evidence-mcp",
        "serverInfo.name must identify this tool, not rmcp's default; got {name:?}"
    );

    // Version should come from `env!("CARGO_PKG_VERSION")` at the
    // macro-expansion callsite, which means the evidence-mcp
    // crate's declared version — not rmcp's. Pin the shape
    // (non-empty + semver-ish) without hardcoding the literal so
    // this test survives version bumps.
    let version = init_resp
        .pointer("/result/serverInfo/version")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("missing result.serverInfo.version: {init_resp}"));
    assert!(
        !version.is_empty() && version.chars().any(|c| c.is_ascii_digit()),
        "version must be non-empty and contain at least one digit; got {version:?}"
    );
    // rmcp's own version (currently 1.5.0) is a common misfire
    // for the default `from_build_env()` expansion — pin against
    // it explicitly so a regression to rmcp defaults fires loud.
    assert_ne!(
        version, "1.5.0",
        "version matches rmcp's 1.5.0 — override is not taking effect"
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

/// TEST-054 selector: a typo'd argument field on an
/// `evidence_check` call (e.g. `workspace` instead of
/// `workspace_path`) must fail at the MCP layer with a clear
/// error — not be silently dropped and fall through to the
/// server's CWD. Pins the `#[serde(deny_unknown_fields)]`
/// contract on `CheckRequest`.
#[test]
fn evidence_check_rejects_unknown_field_typo() {
    let mut frames = init_frames();
    frames.push(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": "evidence_check",
            "arguments": {
                // Typo: missing `_path` suffix. Pre-fix, this
                // would silently deserialize as an empty
                // CheckRequest and fall through to server CWD.
                "workspace": "/tmp/wrong-dir"
            }
        }
    }));

    let responses = session(&frames, 2);
    assert_eq!(responses.len(), 2, "responses: {responses:?}");
    let call_resp = &responses[1];
    // rmcp surfaces serde deserialization failures as a JSON-RPC
    // error object (or a structured tool error). Either shape is
    // acceptable as long as it's NOT a successful run against
    // server CWD. Pin "not a silent-success" by asserting either
    // an error field exists or the terminal is an *_ERROR code.
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
        "expected either a JSON-RPC error, an *_ERROR/*_FAIL terminal, or isError:true \
         on a typo'd field; got a clean success: {call_resp}"
    );
}

/// TEST-054 selector: when a request omits `workspace_path`,
/// the MCP handler MUST prepend a synthetic
/// `MCP_WORKSPACE_FALLBACK` Warning diagnostic at the head of
/// the returned `diagnostics` stream so the agent sees an
/// observable signal before the CLI-produced diagnostics. The
/// signal is the "agents don't silently run on the wrong
/// workspace" contract.
#[test]
fn evidence_check_missing_workspace_path_emits_fallback_signal() {
    // Spawn the server with an empty tempdir as CWD so the
    // fallback "use server CWD" path lands somewhere that
    // fails fast (no Cargo.toml → CLI_INVALID_ARGUMENT)
    // instead of on the Evidence workspace root (where
    // `cargo test --workspace` would run for minutes).
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut frames = init_frames();
    frames.push(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": "evidence_check",
            "arguments": {}
        }
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
        "first diagnostic must be the fallback signal; got {first}",
    );
    assert_eq!(
        first["severity"].as_str(),
        Some("warning"),
        "fallback signal must be Warning severity; got {first}",
    );

    // Summary should include the fallback code with count >= 1.
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
}
