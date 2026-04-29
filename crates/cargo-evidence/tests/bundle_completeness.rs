//! CLI behavior for verify's bundle-completeness cross-check
//! (TEST-`bundle_complete ↔ tool_command_failures ↔ test_summary`).
//!
//! Four scenarios:
//! 1. dev profile + bundle_complete=false + failures → Pass with
//!    `VERIFY_BUNDLE_INCOMPLETE` Warning, exit 0.
//! 2. cert profile + bundle_complete=false + failures → Fail with
//!    `VERIFY_TOOL_COMMANDS_FAILED_SILENTLY`, exit 2.
//! 3. bundle_complete=true + non-empty failures (tamper) → Fail
//!    with `VERIFY_BUNDLE_INCOMPLETELY_CLAIMED`, exit 2.
//! 4. cargo-test failure + test_summary=None → Fail with
//!    `VERIFY_TEST_SUMMARY_ABSENT_ON_FAILED_RUN`, exit 2.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use assert_cmd::Command;
use serde_json::Value;
use tempfile::TempDir;

use evidence_core::bundle::EvidenceIndex;
use evidence_core::hash::{sha256_file, write_sha256sums};

fn cargo_evidence() -> Command {
    #[allow(deprecated)]
    Command::cargo_bin("cargo-evidence").unwrap()
}

/// Build a bundle shaped to test bundle-completeness cross-checks.
/// `bundle_complete` and `tool_command_failures` can be set
/// independently so tamper scenarios are expressible.
/// `test_summary_present` controls whether `index.test_summary`
/// is populated (for the absent-on-failed-run scenario).
fn build_bundle(
    profile: evidence_core::Profile,
    bundle_complete: bool,
    failures: Vec<evidence_core::ToolCommandFailure>,
    test_summary_present: bool,
) -> (TempDir, PathBuf) {
    let tmp = TempDir::new().expect("tempdir");
    let bundle_dir = tmp
        .path()
        .join(format!("{}-20260207-000000Z-aabbccdd", profile));
    fs::create_dir_all(&bundle_dir).unwrap();

    let env_fp = evidence_core::EnvFingerprint {
        profile,
        rustc: "rustc 1.85.0".to_string(),
        cargo: "cargo 1.85.0".to_string(),
        git_sha: "aabbccdd11223344aabbccdd11223344aabbccdd".to_string(),
        git_branch: "main".to_string(),
        git_dirty: false,
        in_nix_shell: false,
        tools: BTreeMap::new(),
        nav_env: BTreeMap::new(),
        llvm_version: None,
        host: evidence_core::Host::Linux {
            arch: "x86_64".to_string(),
            libc: None,
            kernel: None,
        },
        cargo_lock_hash: None,
        rust_toolchain_toml: None,
        rustflags: None,
        target_triple: "x86_64-unknown-linux-gnu".to_string(),
        tool_prerelease: false,
    };
    fs::write(
        bundle_dir.join("env.json"),
        serde_json::to_vec_pretty(&env_fp).unwrap(),
    )
    .unwrap();
    let manifest = env_fp.deterministic_manifest();
    fs::write(
        bundle_dir.join("deterministic-manifest.json"),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();

    let empty_map: BTreeMap<String, String> = BTreeMap::new();
    for name in ["inputs_hashes.json", "outputs_hashes.json"] {
        fs::write(
            bundle_dir.join(name),
            serde_json::to_vec_pretty(&empty_map).unwrap(),
        )
        .unwrap();
    }
    let empty_cmds: Vec<Value> = vec![];
    fs::write(
        bundle_dir.join("commands.json"),
        serde_json::to_vec_pretty(&empty_cmds).unwrap(),
    )
    .unwrap();

    let sha256sums_path = bundle_dir.join("SHA256SUMS");
    write_sha256sums(&bundle_dir, &sha256sums_path).unwrap();
    let content_hash = sha256_file(&sha256sums_path).unwrap();
    let deterministic_hash = sha256_file(&bundle_dir.join("deterministic-manifest.json")).unwrap();

    let test_summary = if test_summary_present {
        Some(evidence_core::TestSummary {
            total: 0,
            passed: 0,
            failed: 0,
            ignored: 0,
            filtered_out: 0,
        })
    } else {
        None
    };

    let index = EvidenceIndex {
        schema_version: evidence_core::schema_versions::INDEX.to_string(),
        boundary_schema_version: evidence_core::schema_versions::BOUNDARY.to_string(),
        trace_schema_version: evidence_core::schema_versions::TRACE.to_string(),
        profile,
        timestamp_rfc3339: "2026-02-07T00:00:00Z".to_string(),
        git_sha: "aabbccdd11223344aabbccdd11223344aabbccdd".to_string(),
        git_branch: "main".to_string(),
        git_dirty: false,
        engine_crate_version: "0.1.0".to_string(),
        engine_git_sha: "eeff001122334455667788990011223344556677".to_string(),
        engine_build_source: "git".to_string(),
        inputs_hashes_file: "inputs_hashes.json".to_string(),
        outputs_hashes_file: "outputs_hashes.json".to_string(),
        commands_file: "commands.json".to_string(),
        env_fingerprint_file: "env.json".to_string(),
        trace_roots: vec![],
        trace_outputs: vec![],
        bundle_complete,
        content_hash,
        deterministic_hash,
        test_summary,
        tool_command_failures: failures,
        dal_map: BTreeMap::new(),
        boundary_policy: evidence_core::BoundaryPolicy::default(),
    };
    fs::write(
        bundle_dir.join("index.json"),
        serde_json::to_vec_pretty(&index).unwrap(),
    )
    .unwrap();

    (tmp, bundle_dir)
}

fn run_verify_jsonl(bundle: &std::path::Path) -> (i32, Vec<Value>) {
    let out = cargo_evidence()
        .args(["evidence", "verify", "--format=jsonl"])
        .arg(bundle)
        .output()
        .expect("spawn");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let diagnostics: Vec<Value> = stdout
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| serde_json::from_str(l).expect("jsonl line parses"))
        .collect();
    (out.status.code().unwrap_or(-1), diagnostics)
}

fn failure(command_name: &str) -> evidence_core::ToolCommandFailure {
    evidence_core::ToolCommandFailure {
        command_name: command_name.to_string(),
        exit_code: 101,
        stderr_tail: "error[E0432]: unresolved import".to_string(),
    }
}

/// Scenario (1): dev profile + bundle_complete=false + recorded
/// failures → verify Pass with `VERIFY_BUNDLE_INCOMPLETE` Warning,
/// exit 0. Local iteration on a broken build must produce an
/// inspectable bundle.
#[test]
fn dev_incomplete_bundle_passes_with_warning() {
    let (_guard, bundle) = build_bundle(
        evidence_core::Profile::Dev,
        false,
        vec![failure("cargo test --workspace")],
        true,
    );
    let (exit, diags) = run_verify_jsonl(&bundle);
    assert_eq!(exit, 0, "dev profile with bundle_complete=false exits 0");
    let codes: Vec<&str> = diags
        .iter()
        .filter_map(|d| d.get("code").and_then(Value::as_str))
        .collect();
    assert!(
        codes.contains(&"VERIFY_BUNDLE_INCOMPLETE"),
        "expected VERIFY_BUNDLE_INCOMPLETE in diagnostics, got {codes:?}",
    );
    assert_eq!(
        codes.last().copied(),
        Some("VERIFY_OK"),
        "terminal should be VERIFY_OK, got {codes:?}",
    );
    let warning_sev = diags
        .iter()
        .find(|d| d.get("code").and_then(Value::as_str) == Some("VERIFY_BUNDLE_INCOMPLETE"))
        .and_then(|d| d.get("severity").and_then(Value::as_str));
    assert_eq!(warning_sev, Some("warning"));
}

/// Scenario (2): cert profile + bundle_complete=false + failures
/// → verify Fail with `VERIFY_TOOL_COMMANDS_FAILED_SILENTLY`,
/// exit 2. Cert/record bundles must not ship with any recorded
/// failures.
#[test]
fn cert_incomplete_bundle_fails_loudly() {
    let (_guard, bundle) = build_bundle(
        evidence_core::Profile::Cert,
        false,
        vec![failure("cargo test --workspace")],
        true,
    );
    let (exit, diags) = run_verify_jsonl(&bundle);
    assert_eq!(exit, 2, "cert profile with failures exits 2");
    let codes: Vec<&str> = diags
        .iter()
        .filter_map(|d| d.get("code").and_then(Value::as_str))
        .collect();
    assert!(
        codes.contains(&"VERIFY_TOOL_COMMANDS_FAILED_SILENTLY"),
        "expected VERIFY_TOOL_COMMANDS_FAILED_SILENTLY, got {codes:?}",
    );
    assert!(
        !codes.contains(&"VERIFY_BUNDLE_INCOMPLETE"),
        "cert path should not emit the dev-profile-only warning",
    );
    assert_eq!(codes.last().copied(), Some("VERIFY_FAIL"));
}

/// Scenario (3): bundle_complete=true + non-empty failures.
/// These two fields are wired strict-consistent at generate
/// time; disagreement is a tamper signal. Fires on ANY profile.
#[test]
fn tampered_bundle_claim_fires_incompletely_claimed() {
    let (_guard, bundle) = build_bundle(
        evidence_core::Profile::Dev,
        true, // tamper: claim complete
        vec![failure("cargo test --workspace")],
        true,
    );
    let (exit, diags) = run_verify_jsonl(&bundle);
    assert_eq!(exit, 2, "tamper fires on any profile");
    let codes: Vec<&str> = diags
        .iter()
        .filter_map(|d| d.get("code").and_then(Value::as_str))
        .collect();
    assert!(
        codes.contains(&"VERIFY_BUNDLE_INCOMPLETELY_CLAIMED"),
        "expected VERIFY_BUNDLE_INCOMPLETELY_CLAIMED, got {codes:?}",
    );
}

/// Scenario (4): cargo-test failure recorded but test_summary is
/// None. The two fields should be wired consistent at generate
/// time (a cargo-test failure records best-effort summary OR
/// absent-but-not-for-cargo-test). Absent on a cargo-test record
/// fires the invariant check.
#[test]
fn cargo_test_failure_without_test_summary_fires_invariant() {
    let (_guard, bundle) = build_bundle(
        evidence_core::Profile::Cert,
        false,
        vec![failure("cargo test --workspace")],
        false, // test_summary absent → invariant violation
    );
    let (_exit, diags) = run_verify_jsonl(&bundle);
    let codes: Vec<&str> = diags
        .iter()
        .filter_map(|d| d.get("code").and_then(Value::as_str))
        .collect();
    assert!(
        codes.contains(&"VERIFY_TEST_SUMMARY_ABSENT_ON_FAILED_RUN"),
        "expected VERIFY_TEST_SUMMARY_ABSENT_ON_FAILED_RUN, got {codes:?}",
    );
}
