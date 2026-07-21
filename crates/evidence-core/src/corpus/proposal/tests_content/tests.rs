//! Semantic content validation tests for the append-only proposal
//! store: every rule fails closed on BOTH the append path (before
//! anything is written) and strict read-back of a hand-written
//! file (TEST-139).

use super::tests_support::*;
use super::{ProposalError, ProposalStore, ProposedRequirementContent};
use crate::corpus::RequirementLayer;

/// The canonical-form positive case: sorted unique verification
/// methods, valid unique `derives_from` targets, and a non-blank
/// title append and read back cleanly (TEST-139). An empty
/// verification-methods list is accepted too, mirroring the
/// review-content contract.
#[test]
fn canonical_content_appends_and_reads_back() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir);
    let mut canonical = content("canonical");
    canonical.verification_methods = vec!["formal review".to_string(), "test".to_string()];
    canonical.derives_from = vec![REQ_A.to_string()];
    let outcome = store
        .append_create_candidate_blocking(SUBMITTER, canonical.clone())
        .expect("canonical content appends");
    let read = ProposalStore::read_proposal_blocking(&outcome.path).expect("reads back");
    let super::ProposalAction::CreateCandidate {
        content: read_content,
        ..
    } = read.proposal.action
    else {
        panic!("create action round-trips");
    };
    assert_eq!(read_content, canonical);

    let mut empty_methods = content("no methods");
    empty_methods.verification_methods = Vec::new();
    store
        .append_create_candidate_blocking(SUBMITTER, empty_methods)
        .expect("an empty verification-methods list is accepted");
}

/// Every append-path rejection below must leave the root empty:
/// validation runs before anything is written (TEST-139).
fn assert_root_empty(dir: &tempfile::TempDir) {
    assert_eq!(entry_count(dir), 0, "a rejected append writes nothing");
}

fn append_err(store: &ProposalStore, content: ProposedRequirementContent) -> ProposalError {
    store
        .append_create_candidate_blocking(SUBMITTER, content)
        .expect_err("invalid content must be rejected")
}

/// Blank titles — empty and whitespace-only — are rejected on
/// append and on read-back (TEST-139).
#[test]
fn blank_title_fails_closed_on_append_and_read() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir);
    for title in ["", "   "] {
        let err = append_err(&store, content(title));
        assert!(
            matches!(err, ProposalError::ProposalContentTitle { .. }),
            "title {title:?} must fail with ProposalContentTitle, got: {err:?}"
        );
        assert_root_empty(&dir);
    }

    let err = read_err(
        &dir,
        "blank-title.toml",
        &doc(&create_action_block()).replace("title = \"t\"", "title = \"  \""),
    );
    assert!(
        matches!(err, ProposalError::ProposalContentTitle { .. }),
        "read-back rejects a blank title, got: {err:?}"
    );
}

/// A revision validates content too: a blank title is rejected
/// before any write, against an otherwise revisable candidate
/// (TEST-139).
#[test]
fn revise_rejects_blank_title_before_writing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir);
    let graph = candidate_graph();
    let expected = current_digest(&graph, REQ_A);
    let err = store
        .append_revise_candidate_blocking(&graph, REQ_A, expected, SUBMITTER, content("  "))
        .expect_err("blank title fails");
    assert!(
        matches!(err, ProposalError::ProposalContentTitle { .. }),
        "got: {err:?}"
    );
    assert_root_empty(&dir);
}

/// Malformed `derives_from` targets fail with the shared native-uid
/// variants on both paths (TEST-139).
#[test]
fn malformed_derives_from_fails_closed_on_append_and_read() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir);

    let mut bad_suffix = content("bad suffix");
    bad_suffix.derives_from = vec!["req_nope".to_string()];
    let err = append_err(&store, bad_suffix);
    assert!(
        matches!(err, ProposalError::NativeUidUuidV4 { .. }),
        "got: {err:?}"
    );
    assert_root_empty(&dir);

    let mut bad_prefix = content("bad prefix");
    bad_prefix.derives_from = vec!["prop_00000000-0000-4000-8000-0000000000aa".to_string()];
    let err = append_err(&store, bad_prefix);
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
    assert_root_empty(&dir);

    let block = format!(
        "[proposal.action]\naction = \"create_candidate\"\ncandidate_uid = \"{REQ_A}\"\n\n\
         [proposal.action.content]\ntitle = \"t\"\nlayer = \"hlr\"\nderives_from = [\"req_nope\"]\n"
    );
    let err = read_err(&dir, "bad-derives.toml", &doc(&block));
    assert!(
        matches!(err, ProposalError::NativeUidUuidV4 { .. }),
        "read-back rejects a malformed derives_from target, got: {err:?}"
    );
}

/// A duplicated `derives_from` target names the duplicate on both
/// paths (TEST-139).
#[test]
fn duplicate_derives_from_fails_closed_on_append_and_read() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir);

    let mut dup = content("duplicate");
    dup.derives_from = vec![REQ_B.to_string(), REQ_A.to_string(), REQ_B.to_string()];
    let err = append_err(&store, dup);
    assert!(
        matches!(
            err,
            ProposalError::ProposalContentDerivesFrom { ref target, .. } if target == REQ_B
        ),
        "the duplicate target is named, got: {err:?}"
    );
    assert_root_empty(&dir);

    let block = format!(
        "[proposal.action]\naction = \"create_candidate\"\ncandidate_uid = \"{REQ_A}\"\n\n\
         [proposal.action.content]\ntitle = \"t\"\nlayer = \"hlr\"\n\
         derives_from = [\"{REQ_A}\", \"{REQ_B}\", \"{REQ_A}\"]\n"
    );
    let err = read_err(&dir, "dup-derives.toml", &doc(&block));
    assert!(
        matches!(
            err,
            ProposalError::ProposalContentDerivesFrom { ref target, .. } if target == REQ_A
        ),
        "read-back names the duplicate, got: {err:?}"
    );
}

/// Verification methods must be sorted as written: the first
/// out-of-order pair is named on both paths (TEST-139).
#[test]
fn unordered_verification_methods_fail_closed_on_append_and_read() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir);

    let mut unordered = content("unordered");
    unordered.verification_methods = vec!["test".to_string(), "analysis".to_string()];
    let err = append_err(&store, unordered);
    assert!(
        matches!(
            err,
            ProposalError::ProposalContentVerificationMethodsOrder {
                ref first,
                ref second,
                ..
            } if first == "test" && second == "analysis"
        ),
        "the first out-of-order pair is named, got: {err:?}"
    );
    assert_root_empty(&dir);

    let block = "[proposal.action]\naction = \"create_candidate\"\n\
                 candidate_uid = \"req_00000000-0000-4000-8000-00000000000a\"\n\n\
                 [proposal.action.content]\ntitle = \"t\"\nlayer = \"hlr\"\n\
                 verification_methods = [\"test\", \"analysis\"]\n";
    let err = read_err(&dir, "unordered-methods.toml", &doc(block));
    assert!(
        matches!(
            err,
            ProposalError::ProposalContentVerificationMethodsOrder { .. }
        ),
        "read-back rejects unordered methods, got: {err:?}"
    );
}

/// Verification methods must be duplicate-free as written: the
/// duplicated method is named on both paths (TEST-139).
#[test]
fn duplicate_verification_methods_fail_closed_on_append_and_read() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir);

    let mut dup = content("duplicate");
    dup.verification_methods = vec![
        "analysis".to_string(),
        "test".to_string(),
        "test".to_string(),
    ];
    let err = append_err(&store, dup);
    assert!(
        matches!(
            err,
            ProposalError::ProposalContentVerificationMethodsDuplicate { ref method, .. }
                if method == "test"
        ),
        "the duplicated method is named, got: {err:?}"
    );
    assert_root_empty(&dir);

    let block = "[proposal.action]\naction = \"create_candidate\"\n\
                 candidate_uid = \"req_00000000-0000-4000-8000-00000000000a\"\n\n\
                 [proposal.action.content]\ntitle = \"t\"\nlayer = \"hlr\"\n\
                 verification_methods = [\"test\", \"test\"]\n";
    let err = read_err(&dir, "dup-methods.toml", &doc(block));
    assert!(
        matches!(
            err,
            ProposalError::ProposalContentVerificationMethodsDuplicate { .. }
        ),
        "read-back rejects duplicated methods, got: {err:?}"
    );
}

/// A derived-layer create carries `safety_impact` through the
/// strict schema: the minted file names the field and strict
/// read-back equals the submitted record (TEST-139).
#[test]
fn create_with_safety_impact_round_trips() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir);
    let mut derived = content("derived candidate");
    derived.layer = RequirementLayer::Derived;
    derived.safety_impact = Some("high".to_string());
    let outcome = store
        .append_create_candidate_blocking(SUBMITTER, derived.clone())
        .expect("append succeeds");
    let bytes = std::fs::read_to_string(&outcome.path).expect("file exists");
    assert!(
        bytes.contains("safety_impact = \"high\""),
        "the minted proposal file carries the field, got:\n{bytes}"
    );

    let read = ProposalStore::read_proposal_blocking(&outcome.path).expect("reads back");
    let super::ProposalAction::CreateCandidate {
        content: read_content,
        ..
    } = read.proposal.action
    else {
        panic!("create action round-trips");
    };
    assert_eq!(read_content, derived);
}

/// A create omitting `safety_impact` round-trips as `None`, and
/// the minted file names no such key (TEST-139).
#[test]
fn create_without_safety_impact_round_trips_as_none() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir);
    let outcome = store
        .append_create_candidate_blocking(SUBMITTER, content("plain candidate"))
        .expect("append succeeds");
    let bytes = std::fs::read_to_string(&outcome.path).expect("file exists");
    assert!(
        !bytes.contains("safety_impact"),
        "an omitted optional field is never serialized, got:\n{bytes}"
    );

    let read = ProposalStore::read_proposal_blocking(&outcome.path).expect("reads back");
    let super::ProposalAction::CreateCandidate {
        content: read_content,
        ..
    } = read.proposal.action
    else {
        panic!("create action round-trips");
    };
    assert!(
        read_content.safety_impact.is_none(),
        "an omitted safety_impact reads back as None"
    );
}
