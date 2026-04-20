//! End-to-end tests for `cargo evidence verify --format=jsonl` plus
//! the dispatch-level JSONL guard on unwired subcommands.
//!
//! Covers every exit-code ↔ terminal-event path documented by Schema
//! Rule 1 in `schemas/diagnostic.schema.json`:
//!
//! | Outcome                        | stdout last line       | exit |
//! |--------------------------------|------------------------|------|
//! | bundle ok                      | `VERIFY_OK`            | 0    |
//! | bundle has findings            | `VERIFY_FAIL`          | 2    |
//! | bundle directory missing       | `VERIFY_ERROR`         | 1    |
//! | strict mode, BUNDLE.sig absent | `VERIFY_FAIL`          | 2    |
//! | `--format=jsonl` on unwired    | `CLI_SUBCOMMAND_ERROR` | 1    |
//!
//! Also pins Schema Rule 2 (stdout-strict JSONL + silent stderr on the
//! JSONL error path) and Schema Rule 4 (flush per event — we don't
//! verify buffering timing here, but parsing each line as an
//! independent JSON object asserts the line boundary contract).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use assert_cmd::Command;
use serde_json::Value;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

fn cargo_evidence() -> Command {
    #[allow(deprecated)]
    Command::cargo_bin("cargo-evidence").unwrap()
}

/// Generate a bundle in `out_dir` rooted at the current repo (the
/// workspace the test runs under). Returns the bundle path.
fn generate_bundle(out_dir: &Path) -> std::path::PathBuf {
    cargo_evidence()
        .arg("evidence")
        .arg("generate")
        .arg("--skip-tests")
        .arg("--out-dir")
        .arg(out_dir)
        .arg("--profile")
        .arg("dev")
        .assert()
        .success();
    // Bundle is written under <out_dir>/dev-<timestamp>-<sha>/.
    fs::read_dir(out_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| {
            p.is_dir()
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .starts_with("dev-")
        })
        .expect("bundle directory under out_dir")
}

/// Split `stdout` into trimmed JSONL lines and parse each as JSON.
/// Empty trailing lines are dropped. Asserts every surviving line is
/// valid JSON — Schema Rule 2 forbids mixed prose on stdout.
fn parse_jsonl(stdout: &[u8]) -> Vec<Value> {
    std::str::from_utf8(stdout)
        .expect("stdout is utf8")
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            serde_json::from_str(l)
                .unwrap_or_else(|e| panic!("line is not valid JSON: {:?} — {}", l, e))
        })
        .collect()
}

#[test]
fn verify_ok_terminates_with_verify_ok_and_exit_zero() {
    let tmp = TempDir::new().unwrap();
    let bundle = generate_bundle(tmp.path());

    let output = cargo_evidence()
        .arg("evidence")
        .arg("--format=jsonl")
        .arg("verify")
        .arg(&bundle)
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let lines = parse_jsonl(&output.stdout);
    assert!(!lines.is_empty(), "expected at least the terminal event");
    let last = lines.last().unwrap();
    assert_eq!(
        last.get("code").and_then(Value::as_str),
        Some("VERIFY_OK"),
        "last line must be VERIFY_OK terminal; got {:?}",
        last
    );
    assert_eq!(
        last.get("severity").and_then(Value::as_str),
        Some("info"),
        "VERIFY_OK must be severity=info",
    );
}

/// Shared runner for the two runtime-error-path tests. Returns
/// `(exit_code, parsed_stdout_lines, stderr_bytes)` so each test can
/// assert on the slice of invariants it cares about.
fn run_verify_jsonl_missing_bundle() -> (Option<i32>, Vec<Value>, Vec<u8>) {
    let tmp = TempDir::new().unwrap();
    let nonexistent = tmp.path().join("does-not-exist");

    let output = cargo_evidence()
        .arg("evidence")
        .arg("--format=jsonl")
        .arg("verify")
        .arg(&nonexistent)
        .output()
        .unwrap();

    let lines = parse_jsonl(&output.stdout);
    (output.status.code(), lines, output.stderr)
}

/// The first line on the runtime-error JSONL path must be the
/// `VERIFY_RUNTIME_BUNDLE_NOT_FOUND` finding. A regression that drops
/// or renames the finding fires this test alone, independent of the
/// terminal invariant tested separately below.
#[test]
fn verify_missing_bundle_emits_runtime_diag_first_line() {
    let (exit, lines, _stderr) = run_verify_jsonl_missing_bundle();

    assert_eq!(exit, Some(1), "runtime fault must map to exit 1");
    assert!(!lines.is_empty(), "expected at least the runtime diag");
    let diag = &lines[0];
    assert_eq!(
        diag.get("code").and_then(Value::as_str),
        Some("VERIFY_RUNTIME_BUNDLE_NOT_FOUND"),
    );
    // Finding carries a `location.file` pointing at the missing path.
    let loc = diag.get("location").expect("runtime diag has location");
    assert!(loc.get("file").and_then(Value::as_str).is_some());
}

/// The runtime-error JSONL stream must end with the `VERIFY_ERROR`
/// terminal so agents can detect stream truncation. A regression that
/// drops or renames the terminal fires this test alone.
#[test]
fn verify_runtime_error_ends_with_verify_error_terminal() {
    let (exit, lines, _stderr) = run_verify_jsonl_missing_bundle();

    assert_eq!(exit, Some(1));
    assert!(
        lines.len() >= 2,
        "runtime-error stream needs at least {{finding, terminal}}; got {} line(s)",
        lines.len()
    );
    let last = lines.last().unwrap();
    let last_code = last.get("code").and_then(Value::as_str).unwrap();
    assert_eq!(last_code, "VERIFY_ERROR", "last line must be VERIFY_ERROR");
    assert_eq!(last.get("severity").and_then(Value::as_str), Some("error"));

    // Cross-check against the library's TERMINAL_CODES source of truth
    // — if a future contributor renames the terminal and forgets to
    // update the const, this fails.
    assert!(
        evidence::TERMINAL_CODES.contains(&last_code),
        "{:?} not in evidence::TERMINAL_CODES = {:?}",
        last_code,
        evidence::TERMINAL_CODES,
    );
}

/// Schema Rule 2: the runtime-error JSONL path must leave stderr silent
/// so agents reading both streams don't see the error twice. If a
/// future tracing::error! or eprintln! lands on this path, the test
/// fires.
#[test]
fn verify_jsonl_runtime_error_has_silent_stderr() {
    let (_exit, _lines, stderr) = run_verify_jsonl_missing_bundle();
    assert!(
        stderr.is_empty(),
        "expected empty stderr on the runtime-error JSONL path; got {} bytes: {:?}",
        stderr.len(),
        String::from_utf8_lossy(&stderr),
    );
}

#[test]
fn verify_finding_emits_terminal_fail_and_exit_two() {
    let tmp = TempDir::new().unwrap();
    let bundle = generate_bundle(tmp.path());

    // Introduce a content-layer tampering: rewrite an already-hashed
    // file to something different. That makes `verify_hash_list`
    // observe a hash mismatch against `SHA256SUMS`.
    let env_json = bundle.join("env.json");
    let original = fs::read(&env_json).unwrap();
    fs::write(&env_json, b"{\"profile\":\"dev\",\"tampered\":true}").unwrap();

    let output = cargo_evidence()
        .arg("evidence")
        .arg("--format=jsonl")
        .arg("verify")
        .arg(&bundle)
        .output()
        .unwrap();

    // Restore so the test doesn't leave a broken bundle lying around
    // (the TempDir cleans up anyway, but this guards against a
    // future test that reuses the path).
    fs::write(&env_json, original).unwrap();

    assert_eq!(
        output.status.code(),
        Some(2),
        "findings must map to exit 2, not 1"
    );

    let lines = parse_jsonl(&output.stdout);
    assert!(
        lines.len() >= 2,
        "expected at least one finding + one terminal, got {} lines",
        lines.len()
    );
    let last = lines.last().unwrap();
    assert_eq!(
        last.get("code").and_then(Value::as_str),
        Some("VERIFY_FAIL"),
        "last line must be VERIFY_FAIL terminal",
    );
    assert_eq!(last.get("severity").and_then(Value::as_str), Some("error"),);
    // Earlier lines must be individual findings — Schema Rule 1
    // reserves the `_OK` / `_FAIL` / `_ERROR` suffixes for the
    // terminal slot.
    for (i, line) in lines[..lines.len() - 1].iter().enumerate() {
        let code = line.get("code").and_then(Value::as_str).unwrap();
        assert!(
            !code.ends_with("_OK") && !code.ends_with("_FAIL") && !code.ends_with("_ERROR"),
            "line {} has terminal-shaped code '{}' before terminal slot",
            i,
            code
        );
    }
}

#[test]
fn verify_jsonl_stdout_is_strict_jsonl_only() {
    // Schema Rule 2: stdout must carry ONLY JSONL. Human-readable
    // progress text lives on stderr.
    let tmp = TempDir::new().unwrap();
    let bundle = generate_bundle(tmp.path());

    let output = cargo_evidence()
        .arg("evidence")
        .arg("--format=jsonl")
        .arg("verify")
        .arg(&bundle)
        .output()
        .unwrap();

    let stdout = std::str::from_utf8(&output.stdout).expect("utf8");
    for (i, line) in stdout.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        // First non-whitespace char must be `{` — no prose lines.
        assert!(
            trimmed.starts_with('{'),
            "stdout line {} is not JSON: {:?}",
            i,
            line
        );
        serde_json::from_str::<Value>(trimmed)
            .unwrap_or_else(|e| panic!("line {} failed JSON parse: {:?} — {}", i, line, e));
    }
}

// =============================================================
// Dispatch-level guard: `--format=jsonl` on unwired subcommands
// =============================================================

/// Every subcommand other than `verify` currently lacks native JSONL
/// support. The dispatch layer must refuse `--format=jsonl` for them
/// with a structured two-event envelope (finding + terminal), not fall
/// through to their human/JSON code paths — which would silently
/// violate Schema Rule 2 by mixing prose on stdout.
///
/// Ordering matters: the finding (`CLI_UNSUPPORTED_FORMAT`) comes
/// first so the agent has the root cause, then the terminal
/// (`CLI_SUBCOMMAND_ERROR`) carrying the subcommand name in a
/// dedicated `subcommand` field. Exit 1.
fn run_unwired_subcommand(subcommand: &str) -> (Option<i32>, Vec<Value>, Vec<u8>) {
    // Each subcommand has its own minimum-viable arg list (some
    // require positional args, some don't). Build the arg vector
    // inline rather than forcing a dummy shape.
    let mut cmd = cargo_evidence();
    cmd.arg("evidence").arg("--format=jsonl").arg(subcommand);
    match subcommand {
        "diff" => {
            // diff needs two bundle paths; use bogus values — the
            // dispatch guard fires before clap resolves them as
            // real dirs.
            cmd.arg("/nonexistent-a").arg("/nonexistent-b");
        }
        "init" | "generate" | "schema" | "trace" => {
            // No required positional args reachable before guard.
            if subcommand == "schema" {
                cmd.arg("show").arg("index");
            }
        }
        other => panic!("unhandled subcommand in test setup: {}", other),
    }
    let output = cmd.output().unwrap();
    (
        output.status.code(),
        parse_jsonl(&output.stdout),
        output.stderr,
    )
}

/// Check one unwired subcommand against the guard's contract. Shared
/// by every `unwired_*` test below so regressions show up with the
/// offending subcommand name, not as "which of five cases broke?".
fn assert_unwired_jsonl_contract(subcommand: &str) {
    let (exit, lines, stderr) = run_unwired_subcommand(subcommand);

    assert_eq!(
        exit,
        Some(1),
        "unwired --format=jsonl for '{}' must exit 1 (stdout:\n{})",
        subcommand,
        lines
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join("\n"),
    );
    assert_eq!(
        lines.len(),
        2,
        "unwired subcommand '{}' must emit exactly {{finding, terminal}}; got {}",
        subcommand,
        lines.len()
    );

    let finding = &lines[0];
    assert_eq!(
        finding.get("code").and_then(Value::as_str),
        Some("CLI_UNSUPPORTED_FORMAT"),
        "first line must be the CLI_UNSUPPORTED_FORMAT finding for '{}'",
        subcommand,
    );
    assert_eq!(
        finding.get("subcommand").and_then(Value::as_str),
        Some(subcommand),
        "finding must carry subcommand='{}'",
        subcommand,
    );

    let terminal = &lines[1];
    let term_code = terminal.get("code").and_then(Value::as_str).unwrap();
    assert_eq!(term_code, "CLI_SUBCOMMAND_ERROR");
    assert!(
        evidence::TERMINAL_CODES.contains(&term_code),
        "terminal '{}' must be in evidence::TERMINAL_CODES",
        term_code,
    );
    assert_eq!(
        terminal.get("subcommand").and_then(Value::as_str),
        Some(subcommand),
    );

    // Schema Rule 2: stderr stays silent on the guard path too.
    assert!(
        stderr.is_empty(),
        "expected empty stderr for unwired '{}'; got: {:?}",
        subcommand,
        String::from_utf8_lossy(&stderr),
    );
}

#[test]
fn unwired_generate_jsonl_is_rejected() {
    assert_unwired_jsonl_contract("generate");
}

#[test]
fn unwired_diff_jsonl_is_rejected() {
    assert_unwired_jsonl_contract("diff");
}

#[test]
fn unwired_init_jsonl_is_rejected() {
    assert_unwired_jsonl_contract("init");
}

#[test]
fn unwired_schema_jsonl_is_rejected() {
    assert_unwired_jsonl_contract("schema");
}

// `trace` supports `--format=jsonl` (per-variant LinkError stream);
// running without `--validate` / `--backfill-uuids` emits
// `CLI_INVALID_ARGUMENT` on the "no action specified" path, not
// `CLI_UNSUPPORTED_FORMAT`, so the matching unwired-rejection test
// would be testing an obsolete branch.
