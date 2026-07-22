//! Unit tests for `evidence_core::verify::bundle`. Covers the
//! verify-time boundary recheck (LLR-072) plus its missing-artifact
//! BVA branch.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use std::collections::BTreeMap;

use super::boundary_recheck::check_boundary_recheck;
use super::*;
use crate::policy::{BoundaryPolicy, Profile};

/// Build a minimal `EvidenceIndex` with a populated `dal_map`
/// (== in-scope set) and a custom `boundary_policy`. Other fields
/// get default-ish placeholders — none of them feed into
/// `check_boundary_recheck`, so the test only has to set the two
/// fields it inspects.
fn idx_with_policy(policy: BoundaryPolicy, in_scope: &[&str]) -> EvidenceIndex {
    let dal_map: BTreeMap<String, String> = in_scope
        .iter()
        .map(|c| ((*c).to_string(), "D".to_string()))
        .collect();
    EvidenceIndex {
        schema_version: "1".to_string(),
        boundary_schema_version: "1".to_string(),
        trace_schema_version: "1".to_string(),
        profile: Profile::Dev,
        timestamp_rfc3339: "2026-04-29T00:00:00Z".to_string(),
        git_sha: "0".repeat(40),
        git_branch: "main".to_string(),
        git_dirty: false,
        engine_crate_version: "0.0.0-test".to_string(),
        engine_git_sha: "0".repeat(40),
        engine_build_source: "git".to_string(),
        inputs_hashes_file: "inputs_hashes.json".to_string(),
        outputs_hashes_file: "outputs_hashes.json".to_string(),
        commands_file: "commands.json".to_string(),
        env_fingerprint_file: "env.json".to_string(),
        trace_roots: Vec::new(),
        trace_outputs: Vec::new(),
        bundle_complete: true,
        content_hash: "0".repeat(64),
        recipe_hash: "0".repeat(64),
        test_summary: None,
        tool_command_failures: Vec::new(),
        dal_map,
        boundary_policy: policy,
        resolution_policy: crate::policy::ResolutionPolicy::LOCKED_OFFLINE,
        completeness: crate::bundle::CompletenessStates::legacy(),
    }
}

fn write_projection_json(bundle: &Path, raw_metadata: &str) {
    let projection =
        crate::cargo_metadata::CargoMetadataProjection::from_raw_metadata(raw_metadata).unwrap();
    let path = bundle.join("cargo_metadata.json");
    std::fs::write(&path, projection.to_canonical_json().unwrap()).unwrap();
}

fn raw_metadata_with_build_rs() -> String {
    serde_json::json!({
        "packages": [{
            "name": "in_scope",
            "id": "path+file:///s#0.1.0",
            "version": "0.1.0",
            "targets": [
                {"kind": ["lib"]},
                {"kind": ["custom-build"]}
            ],
            "links": "libz"
        }],
        "workspace_members": [],
        "resolve": {"nodes": []}
    })
    .to_string()
}

fn raw_metadata_with_proc_macro() -> String {
    serde_json::json!({
        "packages": [{
            "name": "in_scope",
            "id": "path+file:///s#0.1.0",
            "version": "0.1.0",
            "targets": [{"kind": ["proc-macro"]}]
        }],
        "workspace_members": [],
        "resolve": {"nodes": []}
    })
    .to_string()
}

fn raw_metadata_clean() -> String {
    serde_json::json!({
        "packages": [{
            "name": "in_scope",
            "id": "path+file:///s#0.1.0",
            "version": "0.1.0",
            "targets": [{"kind": ["lib"]}]
        }],
        "workspace_members": [],
        "resolve": {"nodes": []}
    })
    .to_string()
}

/// TEST-079 (recheck path): a tampered `cargo_metadata.json` that
/// lists an in-scope package with a custom-build target fires the
/// verify-time recheck.
#[test]
fn tampered_cargo_metadata_fires_recheck() {
    let tmp = tempfile::TempDir::new().unwrap();
    write_projection_json(tmp.path(), &raw_metadata_with_build_rs());

    let policy = BoundaryPolicy {
        no_out_of_scope_deps: false,
        forbid_build_rs: true,
        forbid_proc_macros: false,
    };
    let index = idx_with_policy(policy, &["in_scope"]);
    let mut errors = Vec::new();
    check_boundary_recheck(tmp.path(), &index, &mut errors);

    assert_eq!(errors.len(), 1, "expected one finding, got {errors:?}");
    match &errors[0] {
        VerifyError::BoundaryVerifyForbiddenBuildRs { details } => {
            assert!(
                details.contains("in_scope") && details.contains("libz"),
                "details should name crate + links: {details}"
            );
        }
        other => panic!("wrong variant: {other:?}"),
    }
}

/// TEST-079 (recheck path, proc-macro side): a tampered projection
/// claiming an in-scope proc-macro target fires the recheck.
#[test]
fn tampered_cargo_metadata_fires_proc_macro_recheck() {
    let tmp = tempfile::TempDir::new().unwrap();
    write_projection_json(tmp.path(), &raw_metadata_with_proc_macro());

    let policy = BoundaryPolicy {
        no_out_of_scope_deps: false,
        forbid_build_rs: false,
        forbid_proc_macros: true,
    };
    let index = idx_with_policy(policy, &["in_scope"]);
    let mut errors = Vec::new();
    check_boundary_recheck(tmp.path(), &index, &mut errors);

    assert_eq!(errors.len(), 1);
    assert!(matches!(
        &errors[0],
        VerifyError::BoundaryVerifyForbiddenProcMacro { .. }
    ));
}

/// TEST-079 (BVA): policy claims enforcement but bundle is missing
/// `cargo_metadata.json` — `BOUNDARY_VERIFY_METADATA_MISSING` fires.
#[test]
fn missing_cargo_metadata_fires_metadata_missing() {
    let tmp = tempfile::TempDir::new().unwrap();
    // Deliberately do NOT write cargo_metadata.json.

    let policy = BoundaryPolicy {
        no_out_of_scope_deps: false,
        forbid_build_rs: true,
        forbid_proc_macros: false,
    };
    let index = idx_with_policy(policy, &["in_scope"]);
    let mut errors = Vec::new();
    check_boundary_recheck(tmp.path(), &index, &mut errors);

    assert_eq!(errors.len(), 1);
    assert!(matches!(
        &errors[0],
        VerifyError::BoundaryVerifyMetadataMissing
    ));
}

/// Recheck is a no-op when neither flag is enabled — the bundle
/// didn't claim the policy, so re-running it would be wrong.
#[test]
fn recheck_skipped_when_policy_is_default() {
    let tmp = tempfile::TempDir::new().unwrap();
    write_projection_json(tmp.path(), &raw_metadata_with_build_rs());

    let policy = BoundaryPolicy::default();
    let index = idx_with_policy(policy, &["in_scope"]);
    let mut errors = Vec::new();
    check_boundary_recheck(tmp.path(), &index, &mut errors);
    assert!(errors.is_empty(), "expected no findings: {errors:?}");
}

/// Recheck is also a no-op when `dal_map` is empty (legacy bundle
/// or pre-DAL bundle) — without scoping data the recheck has no
/// safe behavior.
#[test]
fn recheck_skipped_when_dal_map_is_empty() {
    let tmp = tempfile::TempDir::new().unwrap();
    write_projection_json(tmp.path(), &raw_metadata_with_build_rs());

    let policy = BoundaryPolicy {
        no_out_of_scope_deps: false,
        forbid_build_rs: true,
        forbid_proc_macros: false,
    };
    let index = idx_with_policy(policy, &[]);
    let mut errors = Vec::new();
    check_boundary_recheck(tmp.path(), &index, &mut errors);
    assert!(errors.is_empty(), "expected no findings: {errors:?}");
}

/// A clean projection (no build.rs, no proc-macro in in-scope
/// crates) does not fire even when the policy is enabled.
#[test]
fn recheck_passes_on_clean_projection() {
    let tmp = tempfile::TempDir::new().unwrap();
    write_projection_json(tmp.path(), &raw_metadata_clean());

    let policy = BoundaryPolicy {
        no_out_of_scope_deps: false,
        forbid_build_rs: true,
        forbid_proc_macros: true,
    };
    let index = idx_with_policy(policy, &["in_scope"]);
    let mut errors = Vec::new();
    check_boundary_recheck(tmp.path(), &index, &mut errors);
    assert!(errors.is_empty(), "expected no findings: {errors:?}");
}

/// TEST-162 (verify half): re-projecting the recipe manifest from
/// the bundle's recorded inputs passes a clean bundle, catches a
/// tampered inputs file as projection drift, and catches a
/// `recipe_hash` that disagrees with the manifest bytes.
#[test]
fn recipe_manifest_reprojection_detects_tampered_inputs() {
    let tmp = tempfile::TempDir::new().unwrap();
    let bundle = tmp.path();

    let env_fp = crate::env::EnvFingerprint {
        profile: Profile::Dev,
        rustc: "rustc 1.95.0".to_string(),
        cargo: "cargo 1.95.0".to_string(),
        git_sha: "0".repeat(40),
        git_branch: "main".to_string(),
        git_dirty: false,
        in_nix_shell: false,
        tools: BTreeMap::new(),
        nav_env: BTreeMap::new(),
        llvm_version: None,
        host: crate::env::Host::Linux {
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
    std::fs::write(
        bundle.join("env.json"),
        serde_json::to_vec_pretty(&env_fp).unwrap(),
    )
    .unwrap();

    let mut inputs = BTreeMap::new();
    inputs.insert("src/lib.rs".to_string(), "a".repeat(64));
    std::fs::write(
        bundle.join("inputs_hashes.json"),
        serde_json::to_vec_pretty(&inputs).unwrap(),
    )
    .unwrap();
    let commands: Vec<crate::bundle::CommandRecord> = Vec::new();
    std::fs::write(
        bundle.join("commands.json"),
        serde_json::to_vec_pretty(&commands).unwrap(),
    )
    .unwrap();

    let recipe_inputs = crate::env::RecipeInputs::from_bundle_dir(
        bundle,
        crate::policy::ResolutionPolicy::LOCKED_OFFLINE,
    )
    .expect("gather recipe inputs");
    let manifest = env_fp.recipe_manifest(&recipe_inputs);
    std::fs::write(
        bundle.join("deterministic-manifest.json"),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();

    let mut index = idx_with_policy(BoundaryPolicy::default(), &[]);
    index.recipe_hash =
        crate::hash::sha256_file(&bundle.join("deterministic-manifest.json")).unwrap();

    // Clean bundle: the re-projection byte-matches the committed
    // manifest, so no findings.
    let mut errors = Vec::new();
    check_recipe_manifest(bundle, &index, &mut errors).unwrap();
    assert!(
        errors.is_empty(),
        "clean re-projection must pass: {errors:?}"
    );

    // Tamper the inputs file without touching the manifest: the
    // re-projection's inputs_hash moves off the committed value.
    inputs.insert("src/lib.rs".to_string(), "b".repeat(64));
    std::fs::write(
        bundle.join("inputs_hashes.json"),
        serde_json::to_vec_pretty(&inputs).unwrap(),
    )
    .unwrap();
    let mut errors = Vec::new();
    check_recipe_manifest(bundle, &index, &mut errors).unwrap();
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, VerifyError::ManifestProjectionDrift { .. })),
        "tampered inputs must drift the re-projection: {errors:?}"
    );

    // A recipe_hash that disagrees with the manifest bytes fires
    // the 6a mismatch independent of the re-projection.
    index.recipe_hash = "0".repeat(64);
    let mut errors = Vec::new();
    check_recipe_manifest(bundle, &index, &mut errors).unwrap();
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, VerifyError::DeterministicHashMismatch { .. })),
        "wrong recipe_hash must fire the mismatch: {errors:?}"
    );
}
