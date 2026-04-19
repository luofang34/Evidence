//! Integration tests for `cargo evidence check` in bundle mode
//! (PR #46 / LLR-025, TEST-028).
//!
//! Bundle mode is a passthrough to the existing `verify` pipeline —
//! decision 5 of the PR #46 plan calls out `check` as the high-level
//! agent verb and `verify` as the low-level primitive. These tests
//! pin both that passthrough (same wire shape) and the mode-dispatch
//! edge cases.

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

/// Generate a bundle under `out_dir` from the current repo.
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
        .expect("bundle under out_dir")
}

/// TEST-028: `check <bundle>` streams the exact same `VERIFY_*`
/// findings + terminal as `verify --format=jsonl <bundle>`. Bundle
/// mode is a passthrough; wire shape must be literally identical so
/// scripts that consume one keep working with the other.
#[test]
fn check_bundle_mode_matches_verify() {
    let tmp = TempDir::new().unwrap();
    let bundle = generate_bundle(tmp.path());

    // Run verify --format=jsonl
    let verify_out = cargo_evidence()
        .arg("evidence")
        .arg("--format=jsonl")
        .arg("verify")
        .arg(&bundle)
        .output()
        .unwrap();

    // Run check <bundle> (auto-detect → bundle mode)
    let check_out = cargo_evidence()
        .arg("evidence")
        .arg("--format=jsonl")
        .arg("check")
        .arg(&bundle)
        .output()
        .unwrap();

    assert_eq!(
        verify_out.status.code(),
        check_out.status.code(),
        "exit codes must match between verify and check for the same bundle"
    );

    // Each line is a JSON object; compare by `code` sequence to prove
    // the findings + terminal line up. Byte-exact match on stdout is
    // stronger than we need (cmd_check sets `subcommand: "check"` on
    // its own terminal emission but delegates to cmd_verify for the
    // body — the diagnostic sequence is what agents read).
    let verify_codes: Vec<String> = parse_codes(&verify_out.stdout);
    let check_codes: Vec<String> = parse_codes(&check_out.stdout);
    assert_eq!(
        verify_codes, check_codes,
        "diagnostic code sequence must match"
    );
}

fn parse_codes(stdout: &[u8]) -> Vec<String> {
    std::str::from_utf8(stdout)
        .unwrap()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let v: Value = serde_json::from_str(l).expect("jsonl line is valid JSON");
            v.get("code")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string()
        })
        .collect()
}

/// TEST-028 pair: `check --mode=source <bundle>` rejects with a
/// `CLI_INVALID_ARGUMENT` diagnostic rather than silently running
/// the source pipeline against a bundle.
#[test]
fn check_mode_source_on_bundle_is_rejected() {
    let tmp = TempDir::new().unwrap();
    let bundle = generate_bundle(tmp.path());

    let out = cargo_evidence()
        .arg("evidence")
        .arg("--format=jsonl")
        .arg("check")
        .arg("--mode=source")
        .arg(&bundle)
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(2), "mode mismatch should exit 2");
    let codes = parse_codes(&out.stdout);
    assert!(
        codes.contains(&"CLI_INVALID_ARGUMENT".to_string()),
        "expected CLI_INVALID_ARGUMENT in: {:?}",
        codes
    );
    assert_eq!(
        codes.last().map(String::as_str),
        Some("VERIFY_FAIL"),
        "terminal must be VERIFY_FAIL, got: {:?}",
        codes
    );
}

/// TEST-028 pair: `check --mode=bundle <source-tree>` rejects
/// symmetrically.
#[test]
fn check_mode_bundle_on_source_is_rejected() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("Cargo.toml"), "[workspace]\nmembers = []\n").unwrap();

    let out = cargo_evidence()
        .arg("evidence")
        .arg("--format=jsonl")
        .arg("check")
        .arg("--mode=bundle")
        .arg(tmp.path())
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(2));
    let codes = parse_codes(&out.stdout);
    assert!(codes.contains(&"CLI_INVALID_ARGUMENT".to_string()));
    assert_eq!(codes.last().map(String::as_str), Some("VERIFY_FAIL"));
}

/// TEST-028 pair: `check` with no PATH defaults to `.` (current dir).
/// We invoke it from a temp dir containing only a dummy `SHA256SUMS`
/// so auto-dispatch picks bundle mode — enough to prove the default
/// resolves to the cwd without running cargo test.
#[test]
fn check_with_no_path_defaults_to_cwd() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("SHA256SUMS"), "").unwrap();

    // No path arg. Default should resolve to `.`.
    let out = cargo_evidence()
        .arg("evidence")
        .arg("--format=jsonl")
        .arg("check")
        .current_dir(tmp.path())
        .output()
        .unwrap();

    // A dummy (empty) SHA256SUMS isn't a real bundle, so verify will
    // fail — but it will fail via the bundle-mode pipeline, not via
    // CLI_INVALID_ARGUMENT. Proves the path default worked.
    let codes = parse_codes(&out.stdout);
    assert!(
        !codes.contains(&"CLI_INVALID_ARGUMENT".to_string()),
        "default path should resolve to '.' and pick bundle mode (not CLI_INVALID_ARGUMENT): {:?}",
        codes
    );
}
