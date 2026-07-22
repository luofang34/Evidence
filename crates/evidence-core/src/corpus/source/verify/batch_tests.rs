//! Tests for the batch contract: global prerequisite gating,
//! per-head finding isolation, deterministic sorting, and
//! no-mutation (TEST-155).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::fixtures::*;
use super::{SourcePayloadError, SourceVerificationState, verify_effective_sources};
use crate::corpus::{EdgeKind, Node, SourceError, SourceLockError};

/// A graph or lock failure stops the batch before any payload work:
/// the function returns the typed prerequisite error and no
/// per-head entries. Every corpus below is fully provisioned, so if
/// a gate did not stop the batch the call would return `Ok` with
/// per-head findings instead (TEST-155).
#[test]
fn prerequisite_failures_stop_before_payload_work() {
    let digest = digest_of(PAYLOAD_BYTES);

    // Graph validation fails: a dangling supersedes edge.
    let mut revision =
        super::fixtures::revision_node(SRC_1, DOC_1, vendored_material(&digest, VENDORED_PATH));
    revision.edges.push((
        EdgeKind::Supersedes,
        "src_00000000-0000-4000-8000-0000000000ff".to_string(),
    ));
    let corpus = corpus_of(
        vec![Node::SourceRevision(revision)],
        &[(VENDORED_PATH, PAYLOAD_BYTES)],
    );
    let err = verify_effective_sources(corpus.root(), &corpus.graph, &corpus.lock_bytes)
        .expect_err("an invalid graph must stop the batch");
    assert!(
        matches!(err, SourceError::Lock(SourceLockError::InvalidGraph { .. })),
        "graph validation is the first gate, got: {err:?}"
    );

    // Lock parsing fails: malformed TOML.
    let corpus = corpus_of(
        vec![vendored_revision(SRC_1, DOC_1, &digest, VENDORED_PATH)],
        &[(VENDORED_PATH, PAYLOAD_BYTES)],
    );
    let err = verify_effective_sources(corpus.root(), &corpus.graph, b"schema_version = [")
        .expect_err("a malformed lock must stop the batch");
    assert!(
        matches!(err, SourceError::Lock(SourceLockError::Parse { .. })),
        "lock parsing is a global gate, got: {err:?}"
    );

    // Lock canonicality fails: canonical bytes plus a trailing
    // blank line.
    let mut non_canonical = corpus.lock_bytes.clone();
    non_canonical.push(b'\n');
    let err = verify_effective_sources(corpus.root(), &corpus.graph, &non_canonical)
        .expect_err("non-canonical lock bytes must stop the batch");
    assert!(
        matches!(err, SourceError::Lock(SourceLockError::NonCanonical { .. })),
        "lock canonicality is a global gate, got: {err:?}"
    );

    // Graph-lock equality fails: the graph grew a head the lock
    // does not bind.
    let mut nodes = four_document_nodes(&digest);
    nodes.push(hash_only_revision(SRC_5, DOC_5, &"e".repeat(64)));
    let corpus = corpus_of(nodes, &[(VENDORED_PATH, PAYLOAD_BYTES)]);
    let lock_bytes = {
        // The committed lock covers only the four original heads.
        let four = corpus_of(
            four_document_nodes(&digest),
            &[(VENDORED_PATH, PAYLOAD_BYTES)],
        );
        four.lock_bytes
    };
    let err = verify_effective_sources(corpus.root(), &corpus.graph, &lock_bytes)
        .expect_err("a graph/lock disagreement must stop the batch");
    assert!(
        matches!(
            err,
            SourceError::Lock(SourceLockError::Missing { ref document_key })
                if document_key == DOC_5
        ),
        "graph-lock equality is a global gate, got: {err:?}"
    );
}

/// One bad payload never hides findings for later heads: the batch
/// reports the good head, the missing payload, and the digest
/// mismatch side by side (TEST-155).
#[test]
fn one_bad_payload_never_hides_later_findings() {
    let digest = digest_of(PAYLOAD_BYTES);
    let altered = b"altered payload bytes\n";
    let corpus = corpus_of(
        vec![
            vendored_revision(SRC_1, DOC_1, &digest, "sources/doc-1/ok.pdf"),
            vendored_revision(SRC_2, DOC_2, &digest, "sources/doc-2/missing.pdf"),
            vendored_revision(SRC_3, DOC_3, &digest, "sources/doc-3/altered.pdf"),
        ],
        &[
            ("sources/doc-1/ok.pdf", PAYLOAD_BYTES),
            ("sources/doc-3/altered.pdf", altered),
        ],
    );
    let results = verify_effective_sources(corpus.root(), &corpus.graph, &corpus.lock_bytes)
        .expect("prerequisites pass");
    assert_eq!(results.len(), 3, "every head reports, good or bad");
    assert!(
        matches!(
            &results[0].outcome,
            Ok(SourceVerificationState::VerifiedBytes)
        ),
        "the good head still verifies, got: {:?}",
        results[0].outcome
    );
    assert!(
        matches!(
            &results[1].outcome,
            Err(SourcePayloadError::MissingPayload { source_uid, .. })
                if source_uid == SRC_2
        ),
        "the missing payload reports, got: {:?}",
        results[1].outcome
    );
    assert!(
        matches!(
            &results[2].outcome,
            Err(SourcePayloadError::DigestMismatch {
                source_uid,
                actual,
                ..
            }) if source_uid == SRC_3 && actual.as_str() == digest_of(altered)
        ),
        "the digest mismatch reports, got: {:?}",
        results[2].outcome
    );
}

/// Batch results are sorted by document key, then source uid —
/// `effective_source_heads` iterates a `BTreeMap`, so the sort is
/// structural with no re-sort, and a validated graph binds exactly
/// one head per document key (TEST-155).
#[test]
fn batch_results_sort_by_document_key_then_uid() {
    let corpus = corpus_of(
        vec![
            hash_only_revision(SRC_2, "DOC-B", &"b".repeat(64)),
            hash_only_revision(SRC_1, "DOC-A", &"a".repeat(64)),
            hash_only_revision(SRC_3, "DOC-C", &"c".repeat(64)),
        ],
        &[],
    );
    let results = verify_effective_sources(corpus.root(), &corpus.graph, &corpus.lock_bytes)
        .expect("prerequisites pass");
    let order: Vec<(&str, &str)> = results
        .iter()
        .map(|entry| (entry.document_key.as_str(), entry.source_uid.as_str()))
        .collect();
    assert_eq!(
        order,
        vec![("DOC-A", SRC_1), ("DOC-B", SRC_2), ("DOC-C", SRC_3)],
        "insertion order must never reach the result order"
    );
}

/// Verification is read-only: registry records, the committed lock,
/// and payload bytes are byte-identical before and after the batch
/// (TEST-155).
#[test]
fn verification_never_mutates_the_corpus() {
    let corpus = corpus_of(
        four_document_nodes(&digest_of(PAYLOAD_BYTES)),
        &[(VENDORED_PATH, PAYLOAD_BYTES)],
    );
    // Representative registry and committed-lock files beside the
    // payload tree.
    write_payload(
        corpus.root(),
        "sources/records.toml",
        b"schema_version = 1\n",
    );
    std::fs::write(corpus.root().join("sources.lock"), &corpus.lock_bytes).unwrap();

    let before = snapshot_tree(corpus.root());
    let results = verify_effective_sources(corpus.root(), &corpus.graph, &corpus.lock_bytes)
        .expect("prerequisites pass");
    assert_eq!(results.len(), 4);
    assert!(
        results.iter().all(|entry| entry.outcome.is_ok()),
        "every head verifies before the mutation check"
    );
    assert_eq!(
        snapshot_tree(corpus.root()),
        before,
        "verification must not mutate registry, lock, or payload files"
    );
}

/// Read-only byte snapshot of every file beneath `root`, keyed by
/// relative path; symlinks are never followed.
fn snapshot_tree(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    walkdir::WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .map(|entry| entry.unwrap())
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| {
            (
                entry.path().strip_prefix(root).unwrap().to_path_buf(),
                std::fs::read(entry.path()).unwrap(),
            )
        })
        .collect()
}
