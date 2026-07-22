//! Tests for per-capture-mode verification states and offline
//! (network-free) verification (TEST-153).

use super::fixtures::*;
use super::{SourceVerificationState, verify_effective_sources};

/// Vendored, hash-only, external-controlled, and unavailable heads
/// verify to their distinct typed states, each entry bound to its
/// document key and head uid (TEST-153).
#[test]
fn every_capture_mode_reports_its_typed_state() {
    let corpus = corpus_of(
        four_document_nodes(&digest_of(PAYLOAD_BYTES)),
        &[(VENDORED_PATH, PAYLOAD_BYTES)],
    );
    let results = verify_effective_sources(corpus.root(), &corpus.graph, &corpus.lock_bytes)
        .expect("prerequisites pass");
    assert_eq!(results.len(), 4, "one entry per effective head");

    let state_of = |document_key: &str| -> &SourceVerificationState {
        let entry = results
            .iter()
            .find(|entry| entry.document_key == document_key)
            .unwrap_or_else(|| panic!("entry for {document_key} must be present"));
        entry
            .outcome
            .as_ref()
            .unwrap_or_else(|_| panic!("{document_key} must yield a state, not a finding"))
    };
    assert_eq!(state_of(DOC_1), &SourceVerificationState::VerifiedBytes);
    assert_eq!(state_of(DOC_2), &SourceVerificationState::DigestDeclared);
    assert_eq!(
        state_of(DOC_3),
        &SourceVerificationState::ExternallyControlled
    );
    assert_eq!(
        state_of(DOC_4),
        &SourceVerificationState::Unavailable {
            reason: UNAVAILABLE_REASON.to_string(),
        },
        "unavailable material reports its recorded reason"
    );

    let uid_of = |document_key: &str| -> &str {
        &results
            .iter()
            .find(|entry| entry.document_key == document_key)
            .unwrap_or_else(|| panic!("entry for {document_key} must be present"))
            .source_uid
    };
    assert_eq!(uid_of(DOC_1), SRC_1);
    assert_eq!(uid_of(DOC_2), SRC_2);
    assert_eq!(uid_of(DOC_3), SRC_3);
    assert_eq!(uid_of(DOC_4), SRC_4);
}

/// Each state exposes a stable wire string (TEST-153).
#[test]
fn state_wire_strings_are_stable() {
    assert_eq!(
        SourceVerificationState::VerifiedBytes.as_str(),
        "verified_bytes"
    );
    assert_eq!(
        SourceVerificationState::DigestDeclared.as_str(),
        "digest_declared"
    );
    assert_eq!(
        SourceVerificationState::ExternallyControlled.as_str(),
        "externally_controlled"
    );
    assert_eq!(
        SourceVerificationState::Unavailable {
            reason: "any reason".to_string(),
        }
        .as_str(),
        "unavailable"
    );
}

/// Hash-only and external-controlled records whose canonical
/// locations are `https://` URLs verify offline: this module's
/// dependency path carries no HTTP stack — only `std::fs` reads —
/// so completing the batch without any vendored payload present
/// proves validation needed no network access (TEST-153).
#[test]
fn url_locations_complete_offline_without_network() {
    let nodes = vec![
        hash_only_revision(SRC_2, DOC_2, &"b".repeat(64)),
        external_revision(SRC_3, DOC_3, &"c".repeat(64)),
    ];
    // The fixture locations are URLs; verification must never
    // resolve them.
    for node in &nodes {
        let crate::corpus::Node::SourceRevision(revision) = node else {
            panic!("fixture nodes are source revisions");
        };
        assert!(
            revision.canonical_location.starts_with("https://"),
            "the fixture documents a URL-located record"
        );
    }
    let corpus = corpus_of(nodes, &[]);
    assert!(
        !corpus.root().join("sources").exists(),
        "no payload root exists; only vendored heads touch the filesystem"
    );
    let results = verify_effective_sources(corpus.root(), &corpus.graph, &corpus.lock_bytes)
        .expect("prerequisites pass");
    assert_eq!(results.len(), 2);
    assert_eq!(
        results[0].outcome.as_ref().expect("a state"),
        &SourceVerificationState::DigestDeclared
    );
    assert_eq!(
        results[1].outcome.as_ref().expect("a state"),
        &SourceVerificationState::ExternallyControlled
    );
}
