//! Approval boundary end-to-end (TEST-138): a native corpus without
//! gated claims passes under explicit enforcement, and the legacy
//! trace is gated — never grandfathered — when enforcement is
//! explicitly requested.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use std::path::{Path, PathBuf};

use evidence_core::corpus::{
    CorpusIndex, LifecycleEnforcement, RequirementLifecycle, evaluate_all_lifecycles,
    graph_from_trace_files, review_content_digest_v1, validate_approval_boundary,
};
use evidence_core::read_all_trace_files;

const REQ_A: &str = "req_00000000-0000-4000-8000-00000000000a";
const REQ_B: &str = "req_00000000-0000-4000-8000-00000000000b";
const REV_A1: &str = "rev_00000000-0000-4000-8000-0000000000a1";

fn write(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, content).unwrap();
}

fn workspace_trace_root() -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("workspace root")
        .join("cert/trace")
        .to_string_lossy()
        .into_owned()
}

/// A native corpus loaded through `CorpusIndex::load_graph` carries
/// no gated claims at all: native records express neither test nodes
/// nor `modules`/`emits` metadata (a documented non-goal). Explicit
/// enforcement therefore passes even with a candidate requirement,
/// and candidate decomposition stays usable (TEST-138).
#[test]
fn native_corpus_without_gated_claims_passes() {
    let dir = tempfile::tempdir().unwrap();
    write(
        &dir.path().join("reqs/records.toml"),
        r#"
schema_version = 1

[[requirements]]
uid = "req_00000000-0000-4000-8000-00000000000a"
id = "R-A"
layer = "hlr"
title = "reviewed parent"
description = "normative prose of R-A"

[[requirements]]
uid = "req_00000000-0000-4000-8000-00000000000b"
id = "R-B"
layer = "llr"
title = "candidate child"
description = "normative prose of R-B"
derives_from = ["req_00000000-0000-4000-8000-00000000000a"]
"#,
    );
    write(
        &dir.path().join("corpus.toml"),
        "schema_version = 1\nrequirements = [\"reqs/**/*.toml\"]\n",
    );
    let requirements_only =
        CorpusIndex::load_graph(&dir.path().join("corpus.toml")).expect("requirements load");
    let digest_a = review_content_digest_v1(
        &requirements_only
            .review_content(REQ_A)
            .expect("R-A projects content"),
    );

    write(
        &dir.path().join("reviews/records.toml"),
        &format!(
            "schema_version = 1\n\n[[reviews]]\nuid = \"{REV_A1}\"\nid = \"REV-001\"\n\
             requirement_uid = \"{REQ_A}\"\ncontent_schema = 1\n\
             reviewed_content_sha256 = \"{digest}\"\ndecision = \"approve\"\n\
             reviewer = \"alice@example.com\"\nreviewed_at = \"2026-07-01T10:00:00Z\"\n",
            digest = digest_a.as_str(),
        ),
    );
    write(
        &dir.path().join("corpus.toml"),
        "schema_version = 1\nrequirements = [\"reqs/**/*.toml\"]\nreviews = [\"reviews/**/*.toml\"]\n",
    );
    let graph = CorpusIndex::load_graph(&dir.path().join("corpus.toml")).expect("corpus loads");

    let evaluations = evaluate_all_lifecycles(&graph).expect("evaluation succeeds");
    assert_eq!(evaluations[REQ_A].state, RequirementLifecycle::Approved);
    assert_eq!(
        evaluations[REQ_B].state,
        RequirementLifecycle::Candidate,
        "the child stays a candidate — decomposition before approval"
    );

    validate_approval_boundary(&graph, LifecycleEnforcement::Required)
        .expect("no gated claims exist in a native corpus");
}

/// The tool's own legacy `cert/trace` graph has zero review records.
/// When the validator is not called, nothing changes: the graph
/// loads, validates, and evaluates every requirement as a candidate
/// (missing reviews are never implicitly approved). When enforcement
/// IS explicitly requested, the same graph fails closed with one
/// violation per gated claim — legacy graphs are never grandfathered
/// (TEST-138).
#[test]
fn legacy_trace_gates_every_claim_when_requested() {
    let files = read_all_trace_files(&workspace_trace_root()).expect("read own trace");
    let graph = graph_from_trace_files(&files).expect("adapt own trace");

    // Parity: without the validator the legacy graph is untouched.
    graph.validate().expect("own trace graph validates");
    let evaluations = evaluate_all_lifecycles(&graph).expect("evaluation succeeds");
    assert!(
        evaluations
            .values()
            .all(|evaluation| evaluation.state == RequirementLifecycle::Candidate),
        "zero reviews: every requirement is a candidate, never implicitly approved"
    );

    // On explicit request every gated claim fails closed.
    let test_claims: usize = files
        .tests
        .tests
        .iter()
        .map(|test| test.traces_to.len())
        .sum();
    let module_claims = files
        .llr
        .requirements
        .iter()
        .filter(|llr| !llr.modules.is_empty())
        .count();
    let emit_claims = files
        .llr
        .requirements
        .iter()
        .filter(|llr| !llr.emits.is_empty())
        .count();
    let expected = test_claims + module_claims + emit_claims;
    assert!(expected > 0, "the legacy trace carries gated claims");

    let err = validate_approval_boundary(&graph, LifecycleEnforcement::Required)
        .expect_err("zero reviews: every gated claim must fail closed");
    let evidence_core::corpus::ApprovalBoundaryError::Violations { violations } = &err else {
        panic!("expected aggregated violations, got: {err:?}");
    };
    assert_eq!(
        violations.len(),
        expected,
        "one violation per verifies edge, per module claim, and per emitted-code claim"
    );
    assert!(
        violations
            .iter()
            .all(|v| v.state == RequirementLifecycle::Candidate),
        "no legacy node is grandfathered as approved"
    );
    assert!(
        err.to_string().contains("candidate"),
        "the aggregate Display names the lifecycle state"
    );
}
