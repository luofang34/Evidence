//! Tests for `CompletenessStates::derive` + `record_verification_state` —
//! the per-area derivation table (TEST-166).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use super::*;
use crate::policy::{Profile, ResolutionPolicy};

/// The all-good baseline: a full test-running cert-style run
/// with valid trace evidence, in-scope claims, written
/// compliance reports, and a resolved signing key.
fn full_facts() -> CompletenessFacts {
    CompletenessFacts {
        tool_command_failures_empty: true,
        inputs_empty: false,
        outputs_empty: false,
        skip_tests: false,
        test_summary_present: true,
        trace_evidence: Some(TraceEvidenceState::Valid),
        trace_roots_empty: false,
        in_scope_claim: true,
        any_dal_claim: true,
        compliance_reports_complete: true,
        signature_planned: true,
    }
}

#[test]
fn derive_capture_states() {
    let s = CompletenessStates::derive(&full_facts());
    assert_eq!(s.capture, CompletenessState::Complete);

    // A recorded command failure is never capture-complete.
    let mut f = full_facts();
    f.tool_command_failures_empty = false;
    assert_eq!(
        CompletenessStates::derive(&f).capture,
        CompletenessState::Incomplete
    );

    // Empty inputs fail closed even when tests were skipped —
    // the empty-baseline gate runs before the skip gate.
    let mut f = full_facts();
    f.inputs_empty = true;
    f.skip_tests = true;
    assert_eq!(
        CompletenessStates::derive(&f).capture,
        CompletenessState::Incomplete
    );

    // Skipped tests with a sound baseline: no test claim was
    // made, so capture is not applicable.
    let mut f = full_facts();
    f.skip_tests = true;
    f.test_summary_present = false;
    assert_eq!(
        CompletenessStates::derive(&f).capture,
        CompletenessState::NotApplicable
    );

    // A test-running run that lost its summary is incomplete.
    let mut f = full_facts();
    f.test_summary_present = false;
    assert_eq!(
        CompletenessStates::derive(&f).capture,
        CompletenessState::Incomplete
    );
}

#[test]
fn derive_graph_validity_states() {
    let s = CompletenessStates::derive(&full_facts());
    assert_eq!(s.graph_validity, CompletenessState::Complete);

    for state in [
        TraceEvidenceState::NotAdopted {
            missing_roots: vec!["cert/trace".to_string()],
        },
        TraceEvidenceState::Empty,
        TraceEvidenceState::Invalid,
    ] {
        let mut f = full_facts();
        f.trace_evidence = Some(state);
        assert_eq!(
            CompletenessStates::derive(&f).graph_validity,
            CompletenessState::Incomplete
        );
    }

    let mut f = full_facts();
    f.trace_evidence = Some(TraceEvidenceState::NotConfigured);
    assert_eq!(
        CompletenessStates::derive(&f).graph_validity,
        CompletenessState::NotApplicable
    );

    // No classification recorded: unconfigured roots are
    // not-applicable, configured roots are unverifiable.
    let mut f = full_facts();
    f.trace_evidence = None;
    f.trace_roots_empty = true;
    assert_eq!(
        CompletenessStates::derive(&f).graph_validity,
        CompletenessState::NotApplicable
    );
    let mut f = full_facts();
    f.trace_evidence = None;
    assert_eq!(
        CompletenessStates::derive(&f).graph_validity,
        CompletenessState::Unverifiable
    );
}

/// The finalize-time derivation must never report verification
/// completeness optimistically: it always lands Unverifiable,
/// and only the observed immediate outcome (patched in before
/// sealing) can move it to Complete or Incomplete.
#[test]
fn derive_verification_is_never_optimistic() {
    let s = CompletenessStates::derive(&full_facts());
    assert_eq!(s.verification, CompletenessState::Unverifiable);

    // Even a run whose facts are all broken still derives
    // Unverifiable for verification — never a derived Complete
    // and never a guessed Incomplete either.
    let f = CompletenessFacts {
        tool_command_failures_empty: false,
        inputs_empty: true,
        outputs_empty: true,
        skip_tests: false,
        test_summary_present: false,
        trace_evidence: None,
        trace_roots_empty: false,
        in_scope_claim: false,
        any_dal_claim: false,
        compliance_reports_complete: false,
        signature_planned: false,
    };
    let s = CompletenessStates::derive(&f);
    assert_eq!(s.verification, CompletenessState::Unverifiable);
}

#[test]
fn derive_objective_mapping_and_tool_qualification() {
    let s = CompletenessStates::derive(&full_facts());
    assert_eq!(s.objective_mapping, CompletenessState::Complete);
    assert_eq!(s.tool_qualification, CompletenessState::Complete);

    // A missing report for an in-scope crate is incomplete.
    let mut f = full_facts();
    f.compliance_reports_complete = false;
    let s = CompletenessStates::derive(&f);
    assert_eq!(s.objective_mapping, CompletenessState::Incomplete);
    assert_eq!(s.tool_qualification, CompletenessState::Incomplete);

    // No in-scope claim: objective mapping is not applicable;
    // tool qualification keys on the claimed level, not the
    // scope claim.
    let mut f = full_facts();
    f.in_scope_claim = false;
    assert_eq!(
        CompletenessStates::derive(&f).objective_mapping,
        CompletenessState::NotApplicable
    );
    let mut f = full_facts();
    f.any_dal_claim = false;
    assert_eq!(
        CompletenessStates::derive(&f).tool_qualification,
        CompletenessState::NotApplicable
    );
}

#[test]
fn derive_integrity_review_reproducibility() {
    let s = CompletenessStates::derive(&full_facts());
    assert_eq!(s.integrity, CompletenessState::Complete);
    assert_eq!(s.review_approval, CompletenessState::NotApplicable);
    assert_eq!(s.reproducibility, CompletenessState::Complete);

    let mut f = full_facts();
    f.signature_planned = false;
    assert_eq!(
        CompletenessStates::derive(&f).integrity,
        CompletenessState::NotApplicable
    );

    // Zero captured outputs: not applicable for a skip-tests
    // run, incomplete for a test-running one.
    let mut f = full_facts();
    f.outputs_empty = true;
    f.skip_tests = true;
    assert_eq!(
        CompletenessStates::derive(&f).reproducibility,
        CompletenessState::NotApplicable
    );
    let mut f = full_facts();
    f.outputs_empty = true;
    assert_eq!(
        CompletenessStates::derive(&f).reproducibility,
        CompletenessState::Incomplete
    );
}

/// A legacy `index.json` without the `completeness` key
/// deserializes with every state Unverifiable.
#[test]
fn legacy_index_deserializes_all_unverifiable() {
    let legacy = serde_json::json!({
        "schema_version": "1.0.0",
        "boundary_schema_version": "1.0.0",
        "trace_schema_version": "1.0.0",
        "profile": "dev",
        "timestamp_rfc3339": "2024-01-01T00:00:00Z",
        "git_sha": "abc123",
        "git_branch": "main",
        "git_dirty": false,
        "engine_crate_version": "0.1.0",
        "engine_git_sha": "abc123",
        "inputs_hashes_file": "inputs_hashes.json",
        "outputs_hashes_file": "outputs_hashes.json",
        "commands_file": "commands.json",
        "env_fingerprint_file": "env.json",
        "trace_roots": [],
        "trace_outputs": [],
        "bundle_complete": true,
        "content_hash": "deadbeef".repeat(8),
        "recipe_hash": "cafebabe".repeat(8),
    });
    let idx: super::super::EvidenceIndex =
        serde_json::from_value(legacy).expect("legacy index parses");
    assert_eq!(idx.completeness, CompletenessStates::legacy());
    assert_eq!(idx.completeness.capture, CompletenessState::Unverifiable);
    assert_eq!(
        idx.completeness.verification,
        CompletenessState::Unverifiable
    );
}

/// The pre-sealing patch rewrites only `verification` on an
/// existing index and round-trips every other field.
#[test]
fn record_verification_state_patches_index() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let index = super::super::EvidenceIndex {
        schema_version: crate::schema_versions::INDEX.to_string(),
        boundary_schema_version: crate::schema_versions::BOUNDARY.to_string(),
        trace_schema_version: crate::schema_versions::TRACE.to_string(),
        profile: Profile::Dev,
        timestamp_rfc3339: "2026-01-01T00:00:00Z".to_string(),
        git_sha: "0".repeat(40),
        git_branch: "main".to_string(),
        git_dirty: false,
        engine_crate_version: "0.1.0".to_string(),
        engine_git_sha: "0".repeat(40),
        engine_build_source: "git".to_string(),
        inputs_hashes_file: "inputs_hashes.json".to_string(),
        outputs_hashes_file: "outputs_hashes.json".to_string(),
        commands_file: "commands.json".to_string(),
        env_fingerprint_file: "env.json".to_string(),
        trace_roots: vec![],
        trace_outputs: vec![],
        bundle_complete: true,
        content_hash: "0".repeat(64),
        recipe_hash: "1".repeat(64),
        test_summary: None,
        tool_command_failures: Vec::new(),
        dal_map: std::collections::BTreeMap::new(),
        boundary_policy: crate::policy::BoundaryPolicy::default(),
        resolution_policy: ResolutionPolicy::LockedOffline,
        completeness: CompletenessStates::legacy(),
    };
    std::fs::write(
        tmp.path().join("index.json"),
        serde_json::to_vec_pretty(&index).expect("serialize"),
    )
    .expect("write index");

    record_verification_state(tmp.path(), CompletenessState::Complete).expect("patch succeeds");
    let bytes = std::fs::read(tmp.path().join("index.json")).expect("read back");
    let patched: super::super::EvidenceIndex =
        serde_json::from_slice(&bytes).expect("patched parses");
    assert_eq!(
        patched.completeness.verification,
        CompletenessState::Complete
    );
    // Every other state is untouched by the patch.
    assert_eq!(
        patched.completeness.capture,
        CompletenessState::Unverifiable
    );
    assert_eq!(patched.content_hash, "0".repeat(64));

    // A missing index surfaces a typed error, not a panic.
    let missing = tempfile::TempDir::new().expect("tempdir");
    let outcome = record_verification_state(missing.path(), CompletenessState::Complete);
    assert!(matches!(outcome, Err(CompletenessError::Io { .. })));
}
