//! Revision-guard and baseline-immutability tests for the
//! append-only proposal store (TEST-140).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::tests_support::*;
use super::{
    ProposalError, ProposalStore, RequirementLifecycle, ReviewContentDigest, evaluate_lifecycle,
};
use crate::corpus::{CorpusIndex, EdgeKind, LifecycleError};

/// A malformed graph fails the revision guard before any write,
/// and the lifecycle error is preserved as the typed source —
/// never collapsed into "target missing" (TEST-140).
#[test]
fn revise_with_invalid_graph_preserves_lifecycle_source() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir);
    // The requirement node exists, so the target-missing check
    // passes, but its DerivesFrom edge dangles: whole-graph
    // validation fails inside lifecycle evaluation.
    let mut req = requirement(REQ_A, "prose v1");
    req.edges = vec![(
        EdgeKind::DerivesFrom,
        "req_00000000-0000-4000-8000-0000000000ff".to_string(),
    )];
    let graph = graph_with(req, Vec::new());
    let expected = current_digest(&graph, REQ_A);
    let err = revise(&store, &graph, REQ_A, expected).expect_err("invalid graph fails");
    let ProposalError::ProposalLifecycleEvaluation(source) = err else {
        panic!("expected ProposalLifecycleEvaluation, got: {err:?}");
    };
    assert!(
        matches!(*source, LifecycleError::InvalidGraph(_)),
        "the original lifecycle error travels as the typed source, got: {source:?}"
    );
    assert_eq!(entry_count(&dir), 0, "a rejected revision writes nothing");
}

/// Optimistic concurrency: a stale expected digest fails with the
/// uid and both digests (TEST-140).
#[test]
fn revise_with_stale_digest_fails_with_typed_mismatch() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir);
    let graph = candidate_graph();
    let stale = digest_of_prose("prose v0");
    let err = revise(&store, &graph, REQ_A, stale.clone()).expect_err("stale digest fails");
    let ProposalError::ProposalDigestMismatch {
        uid,
        expected,
        actual,
    } = err
    else {
        panic!("expected ProposalDigestMismatch, got: {err:?}");
    };
    assert_eq!(uid, REQ_A);
    assert_eq!(expected, stale);
    assert_eq!(actual, current_digest(&graph, REQ_A));
}

/// Approved, rejected, and stale requirements reject revisions
/// with the lifecycle state named — a proposal can never demote
/// approved content (TEST-140).
#[test]
fn revise_of_approved_requirement_is_rejected() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir);
    let graph = approved_graph();
    let expected = current_digest(&graph, REQ_A);
    let err = revise(&store, &graph, REQ_A, expected).expect_err("approved target fails");
    assert!(
        matches!(
            err,
            ProposalError::ProposalLifecycle {
                state: RequirementLifecycle::Approved,
                ..
            }
        ),
        "approved content can never be demoted by a proposal, got: {err:?}"
    );
}

#[test]
fn revise_of_rejected_requirement_is_rejected() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir);
    let graph = rejected_graph();
    let expected = current_digest(&graph, REQ_A);
    let err = revise(&store, &graph, REQ_A, expected).expect_err("rejected target fails");
    assert!(
        matches!(
            err,
            ProposalError::ProposalLifecycle {
                state: RequirementLifecycle::Rejected,
                ..
            }
        ),
        "got: {err:?}"
    );
}

#[test]
fn revise_of_stale_requirement_is_rejected() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir);
    let graph = stale_graph();
    let expected = current_digest(&graph, REQ_A);
    let err = revise(&store, &graph, REQ_A, expected).expect_err("stale target fails");
    assert!(
        matches!(
            err,
            ProposalError::ProposalLifecycle {
                state: RequirementLifecycle::Stale,
                ..
            }
        ),
        "got: {err:?}"
    );
}

/// A revision naming no requirement fails closed (TEST-140).
#[test]
fn revise_of_missing_requirement_fails_closed() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir);
    let graph = candidate_graph();
    let missing = "req_00000000-0000-4000-8000-0000000000ff";
    let err = revise(
        &store,
        &graph,
        missing,
        ReviewContentDigest::from_hex(DIGEST).expect("digest"),
    )
    .expect_err("missing target fails");
    assert!(
        matches!(err, ProposalError::ProposalTargetMissing { ref uid } if uid == missing),
        "got: {err:?}"
    );
}

/// A malformed base uid is rejected before any graph lookup
/// (TEST-140).
#[test]
fn revise_with_malformed_base_uid_fails_closed() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir);
    let graph = candidate_graph();
    let digest = ReviewContentDigest::from_hex(DIGEST).expect("digest");

    let err = revise(&store, &graph, "req_not-a-uuid", digest.clone()).expect_err("fails");
    assert!(
        matches!(err, ProposalError::NativeUidUuidV4 { .. }),
        "got: {err:?}"
    );
    let err = revise(
        &store,
        &graph,
        "prop_00000000-0000-4000-8000-0000000000aa",
        digest,
    )
    .expect_err("fails");
    assert!(
        matches!(
            err,
            ProposalError::NativeUidPrefix {
                expected: "req_",
                ..
            }
        ),
        "got: {err:?}"
    );
}

/// Byte-identical baseline (TEST-140): a tempdir corpus with
/// requirement and review files is snapshotted, proposals append
/// into a SEPARATE root, and every corpus file is unchanged.
#[test]
fn append_leaves_corpus_baseline_byte_identical() {
    let dir = tempfile::tempdir().expect("tempdir");
    let corpus = dir.path().join("corpus");
    write(
        &corpus.join("reqs/records.toml"),
        &format!(
            "schema_version = 1\n\n\
             [[requirements]]\nuid = \"{REQ_A}\"\nid = \"R-A\"\nlayer = \"hlr\"\n\
             title = \"reviewed parent\"\ndescription = \"prose v1\"\n\n\
             [[requirements]]\nuid = \"{REQ_B}\"\nid = \"R-B\"\nlayer = \"llr\"\n\
             title = \"candidate child\"\ndescription = \"child prose\"\nderives_from = [\"{REQ_A}\"]\n"
        ),
    );
    write(
        &corpus.join("corpus.toml"),
        "schema_version = 1\nrequirements = [\"reqs/**/*.toml\"]\n",
    );
    let requirements_only =
        CorpusIndex::load_graph(&corpus.join("corpus.toml")).expect("requirements load");
    let digest_a = current_digest(&requirements_only, REQ_A);
    let digest_b = current_digest(&requirements_only, REQ_B);
    write(
        &corpus.join("reviews/records.toml"),
        &format!(
            "schema_version = 1\n\n\
             [[reviews]]\nuid = \"{REV_1}\"\nid = \"REV-001\"\n\
             requirement_uid = \"{REQ_A}\"\ncontent_schema = 1\n\
             reviewed_content_sha256 = \"{digest_a}\"\ndecision = \"approve\"\n\
             reviewer = \"alice@example.com\"\nreviewed_at = \"2026-07-01T10:00:00Z\"\n"
        ),
    );
    write(
        &corpus.join("corpus.toml"),
        "schema_version = 1\nrequirements = [\"reqs/**/*.toml\"]\nreviews = [\"reviews/**/*.toml\"]\n",
    );
    let graph = CorpusIndex::load_graph(&corpus.join("corpus.toml")).expect("corpus loads");
    assert_eq!(
        evaluate_lifecycle(&graph, REQ_A).expect("evaluate").state,
        RequirementLifecycle::Approved
    );

    let proposals = dir.path().join("proposals");
    std::fs::create_dir(&proposals).expect("mkdir proposals");
    let store = ProposalStore::new(&proposals).expect("store opens");

    let before = snapshot(&corpus);
    let created = store
        .append_create_candidate_blocking(SUBMITTER, content("brand new"))
        .expect("create appends");
    let revised = revise(&store, &graph, REQ_B, digest_b).expect("revise appends");
    let after = snapshot(&corpus);

    assert_eq!(before, after, "the corpus baseline is byte-identical");
    let proposal_root = proposals.canonicalize().expect("canonical root");
    assert!(
        created.path.starts_with(&proposal_root) && revised.path.starts_with(&proposal_root),
        "proposals land only beneath the proposal root"
    );
    let reloaded = CorpusIndex::load_graph(&corpus.join("corpus.toml")).expect("corpus reloads");
    assert_eq!(reloaded, graph, "the loaded graph is unchanged");
}

fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut map = BTreeMap::new();
    for entry in walkdir::WalkDir::new(root).follow_links(false) {
        let entry = entry.expect("walk entry");
        if entry.file_type().is_file() {
            map.insert(
                entry.path().to_path_buf(),
                std::fs::read(entry.path()).expect("read snapshot file"),
            );
        }
    }
    map
}
