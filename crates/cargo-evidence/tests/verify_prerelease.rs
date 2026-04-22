//! CLI severity-partition behavior on bundles with
//! `env.json.tool_prerelease = true` (TEST-049).
//!
//! Library-side `VerifyError::PrereleaseToolDetected` propagation
//! is covered in `evidence-core/tests/verify_prerelease.rs`. This
//! file exercises the CLI decisions in `cmd_verify_jsonl`:
//!
//! - Dev-profile bundle → code emitted at Warning severity, stream
//!   terminates `VERIFY_OK`, exit 0. The bundle author gets the
//!   signal; the command doesn't block.
//! - Cert/record-profile bundle → code stays Error, stream
//!   terminates `VERIFY_FAIL`, exit 2. Cert bundles from
//!   pre-release tools are not valid audit evidence.

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

fn build_prerelease_bundle(profile: evidence_core::Profile) -> (TempDir, PathBuf) {
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
        tool_prerelease: true,
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
    let empty_cmds: Vec<serde_json::Value> = vec![];
    fs::write(
        bundle_dir.join("commands.json"),
        serde_json::to_vec_pretty(&empty_cmds).unwrap(),
    )
    .unwrap();

    let sha256sums_path = bundle_dir.join("SHA256SUMS");
    write_sha256sums(&bundle_dir, &sha256sums_path).unwrap();
    let content_hash = sha256_file(&sha256sums_path).unwrap();
    let deterministic_hash = sha256_file(&bundle_dir.join("deterministic-manifest.json")).unwrap();

    let index = EvidenceIndex {
        schema_version: evidence_core::schema_versions::INDEX.to_string(),
        boundary_schema_version: evidence_core::schema_versions::BOUNDARY.to_string(),
        trace_schema_version: evidence_core::schema_versions::TRACE.to_string(),
        profile,
        timestamp_rfc3339: "2026-02-07T00:00:00Z".to_string(),
        git_sha: "aabbccdd11223344aabbccdd11223344aabbccdd".to_string(),
        git_branch: "main".to_string(),
        git_dirty: false,
        engine_crate_version: "0.1.0-pre.1".to_string(),
        engine_git_sha: "eeff001122334455667788990011223344556677".to_string(),
        engine_build_source: "git".to_string(),
        inputs_hashes_file: "inputs_hashes.json".to_string(),
        outputs_hashes_file: "outputs_hashes.json".to_string(),
        commands_file: "commands.json".to_string(),
        env_fingerprint_file: "env.json".to_string(),
        trace_roots: vec![],
        trace_outputs: vec![],
        bundle_complete: true,
        content_hash,
        deterministic_hash,
        test_summary: None,
        tool_command_failures: Vec::new(),
        dal_map: BTreeMap::new(),
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
    let exit = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let diags: Vec<Value> = stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("JSON object per line"))
        .collect();
    (exit, diags)
}

/// Plain-text mode variant: runs `verify` with neither `--format=jsonl`
/// nor `--json`, captures exit code + stdout + stderr. Used by the
/// pair of tests below that pin the default text path's behavior,
/// ensuring it mirrors the JSONL path's profile-aware severity split.
fn run_verify_plain(bundle: &std::path::Path) -> (i32, String, String) {
    let out = cargo_evidence()
        .args(["evidence", "verify"])
        .arg(bundle)
        .output()
        .expect("spawn");
    let exit = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    (exit, stdout, stderr)
}

#[test]
fn dev_profile_warning_passes() {
    let (_tmp, bundle) = build_prerelease_bundle(evidence_core::Profile::Dev);
    let (exit, diags) = run_verify_jsonl(&bundle);
    assert_eq!(exit, 0, "dev profile must exit 0; diags={:?}", diags);

    let prerelease = diags
        .iter()
        .find(|d| d["code"].as_str() == Some("VERIFY_PRERELEASE_TOOL"))
        .expect("stream must include VERIFY_PRERELEASE_TOOL");
    assert_eq!(
        prerelease["severity"].as_str(),
        Some("warning"),
        "dev profile must downgrade code to warning severity; got {:?}",
        prerelease["severity"]
    );

    let codes: Vec<&str> = diags.iter().map(|d| d["code"].as_str().unwrap()).collect();
    assert_eq!(
        codes.last().copied(),
        Some("VERIFY_OK"),
        "dev stream must terminate with VERIFY_OK; got codes={:?}",
        codes
    );
}

#[test]
fn cert_profile_blocks_with_verify_fail() {
    let (_tmp, bundle) = build_prerelease_bundle(evidence_core::Profile::Cert);
    let (exit, diags) = run_verify_jsonl(&bundle);
    assert_eq!(exit, 2, "cert profile must exit 2; diags={:?}", diags);

    let prerelease = diags
        .iter()
        .find(|d| d["code"].as_str() == Some("VERIFY_PRERELEASE_TOOL"))
        .expect("stream must include VERIFY_PRERELEASE_TOOL");
    assert_eq!(
        prerelease["severity"].as_str(),
        Some("error"),
        "cert profile keeps code at error severity; got {:?}",
        prerelease["severity"]
    );

    let codes: Vec<&str> = diags.iter().map(|d| d["code"].as_str().unwrap()).collect();
    assert_eq!(
        codes.last().copied(),
        Some("VERIFY_FAIL"),
        "cert stream must terminate with VERIFY_FAIL; got codes={:?}",
        codes
    );
}

/// Plain-text path must mirror the JSONL path's profile-aware
/// severity split: dev-profile pre-release bundle → pass with a
/// stderr warning + exit 0. The default output path is what CI
/// workflows and ad-hoc shell callers hit when they don't pass
/// `--format=jsonl`; if the two paths disagreed, a bundle that
/// passes the JSONL path would fail the plain-text path for the
/// same inputs.
#[test]
fn plain_text_dev_profile_passes_with_warning() {
    let (_tmp, bundle) = build_prerelease_bundle(evidence_core::Profile::Dev);
    let (exit, stdout, stderr) = run_verify_plain(&bundle);
    assert_eq!(
        exit, 0,
        "dev profile plain-text verify must exit 0; stdout={stdout} stderr={stderr}",
    );
    assert!(
        stdout.contains("PASS") || stdout.contains("pass"),
        "stdout must indicate pass; got: {stdout}",
    );
    assert!(
        stderr.contains("pre-release") || stderr.contains("prerelease"),
        "stderr must carry a pre-release warning; got: {stderr}",
    );
    // The library must NOT leak `tracing::error!("VERIFY ERROR:
    // …")` through to stderr on a downgrade-to-warning path —
    // the CLI partition owns severity presentation, and a
    // library-layer error log fires before the partition runs.
    // Seeing `ERROR` from tracing_subscriber on a Warning-outcome
    // run is a regression.
    assert!(
        !stderr.contains("VERIFY ERROR"),
        "stderr must NOT contain 'VERIFY ERROR' on a dev-profile warning path \
         — the library's log leaked past the CLI severity partition; got: {stderr}",
    );
    assert!(
        !stderr.contains("ERROR"),
        "stderr must NOT contain 'ERROR' on a warning-outcome run; got: {stderr}",
    );
}

/// Plain-text path on cert/record profile stays an error — the
/// partition downgrades dev only. Cert bundles from pre-release
/// tools remain rejected regardless of output format.
#[test]
fn plain_text_cert_profile_still_fails() {
    let (_tmp, bundle) = build_prerelease_bundle(evidence_core::Profile::Cert);
    let (exit, _stdout, stderr) = run_verify_plain(&bundle);
    assert_ne!(
        exit, 0,
        "cert profile plain-text verify must exit non-zero on pre-release bundle; stderr={stderr}",
    );
    assert!(
        stderr.contains("pre-release") || stderr.contains("prerelease"),
        "stderr must mention pre-release as the refusal reason; got: {stderr}",
    );
}
