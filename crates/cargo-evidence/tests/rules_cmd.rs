//! Integration tests for `cargo evidence rules [--json]`.
//!
//! Pins the self-describe surface — the committed shape agents and
//! MCP consume. Also asserts `--format=jsonl` is rejected
//! (rules emits a single JSON blob, not a JSONL stream, so it would
//! violate Schema Rule 2).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use assert_cmd::Command;
use serde_json::Value;

fn cargo_evidence() -> Command {
    #[allow(deprecated)]
    Command::cargo_bin("cargo-evidence").unwrap()
}

/// The committed JSON shape MUST match what `evidence_core::rules_json()`
/// produces. Re-running through the CLI + parse catches any drift
/// between the library helper and the CLI wiring.
#[test]
fn rules_json_matches_rules_json_helper() {
    let out = cargo_evidence()
        .args(["evidence", "rules", "--json"])
        .output()
        .expect("spawn");
    assert!(out.status.success(), "rules --json must exit 0");
    let cli_stdout = String::from_utf8(out.stdout).expect("valid utf-8");

    let from_cli: Value = serde_json::from_str(&cli_stdout).expect("CLI output parses");
    let from_lib: Value =
        serde_json::from_str(&evidence_core::rules_json()).expect("library output parses");
    assert_eq!(
        from_cli, from_lib,
        "CLI rules --json diverged from evidence_core::rules_json()"
    );
}

/// `rules --json` length must equal `RULES.len()` — catches the case
/// where the serializer silently drops entries.
#[test]
fn rules_json_length_matches_rules_const() {
    let out = cargo_evidence()
        .args(["evidence", "rules", "--json"])
        .output()
        .expect("spawn");
    let stdout = String::from_utf8(out.stdout).expect("valid utf-8");
    let v: Value = serde_json::from_str(&stdout).expect("parses");
    let arr = v.as_array().expect("top-level array");
    assert_eq!(arr.len(), evidence_core::RULES.len());
}

/// Human-mode `rules` exits 0 and emits a table header we can pin
/// minimally. This is a smoke test — the exact layout may change
/// without breaking the human mode contract.
#[test]
fn rules_human_mode_exits_zero_and_prints_header() {
    let out = cargo_evidence()
        .args(["evidence", "rules"])
        .output()
        .expect("spawn");
    assert!(
        out.status.success(),
        "rules (human mode) must exit 0, got {:?}",
        out.status
    );
    let stdout = String::from_utf8(out.stdout).expect("valid utf-8");
    assert!(stdout.contains("CODE"), "table header missing CODE column");
    assert!(
        stdout.contains("SEVERITY"),
        "table header missing SEVERITY column"
    );
    assert!(
        stdout.contains("DOMAIN"),
        "table header missing DOMAIN column"
    );
    assert!(
        stdout.contains(&format!("{} rule(s) total", evidence_core::RULES.len())),
        "total count line missing or wrong"
    );
}

/// `--format=jsonl` on `rules` is explicitly rejected. Pins both
/// JSONL events and the exit code.
#[test]
fn rules_rejects_format_jsonl_with_two_terminal_events() {
    let out = cargo_evidence()
        .args(["evidence", "--format=jsonl", "rules", "--json"])
        .output()
        .expect("spawn");
    assert_eq!(
        out.status.code(),
        Some(1),
        "unsupported format path must exit 1 (EXIT_ERROR)"
    );

    let stdout = String::from_utf8(out.stdout).expect("valid utf-8");
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(
        lines.len(),
        2,
        "expected exactly 2 JSONL lines, got {}",
        lines.len()
    );

    let first: Value = serde_json::from_str(lines[0]).expect("first line is JSON");
    assert_eq!(first["code"], "CLI_UNSUPPORTED_FORMAT");
    assert_eq!(first["subcommand"], "rules");

    let second: Value = serde_json::from_str(lines[1]).expect("second line is JSON");
    assert_eq!(second["code"], "CLI_SUBCOMMAND_ERROR");
    assert_eq!(second["subcommand"], "rules");
}

/// Every code the manifest advertises must decode back to a known
/// severity + domain — pins that the JSON shape matches the library
/// enum variants end-to-end.
#[test]
fn rules_json_every_entry_has_known_severity_and_domain() {
    let out = cargo_evidence()
        .args(["evidence", "rules", "--json"])
        .output()
        .expect("spawn");
    let stdout = String::from_utf8(out.stdout).expect("valid utf-8");
    let arr: Vec<Value> = serde_json::from_str(&stdout).expect("parses");
    for entry in &arr {
        let sev = entry["severity"].as_str().expect("severity is string");
        assert!(
            matches!(sev, "info" | "warning" | "error"),
            "unknown severity '{}'",
            sev
        );
        let dom = entry["domain"].as_str().expect("domain is string");
        assert!(
            matches!(
                dom,
                "boundary"
                    | "bundle"
                    | "cli"
                    | "cmd"
                    | "doctor"
                    | "env"
                    | "floors"
                    | "git"
                    | "hash"
                    | "policy"
                    | "req"
                    | "schema"
                    | "sign"
                    | "trace"
                    | "verify"
            ),
            "unknown domain '{}'",
            dom
        );
    }
}
