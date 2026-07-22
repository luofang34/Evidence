//! Tests for vendored payload resolution and hashing beneath the
//! fixed payload root (TEST-154).

use super::fixtures::*;
use super::{
    SourcePayloadError, SourceVerificationState, resolve_vendored_path, verify_effective_sources,
    verify_vendored_head,
};
use crate::corpus::{LockAvailability, LockCaptureMode, SourceContentDigest, SourceLockEntry};

/// A vendored head whose on-disk bytes match the record and lock
/// digest verifies; nested path components resolve (TEST-154).
#[test]
fn matching_vendored_bytes_verify() {
    let nested = "sources/doc-1/nested/deep/rev-c.pdf";
    let corpus = corpus_of(
        vec![vendored_revision(
            SRC_1,
            DOC_1,
            &digest_of(PAYLOAD_BYTES),
            nested,
        )],
        &[(nested, PAYLOAD_BYTES)],
    );
    let results = verify_effective_sources(corpus.root(), &corpus.graph, &corpus.lock_bytes)
        .expect("prerequisites pass");
    assert_eq!(results.len(), 1);
    assert_eq!(
        results[0].outcome.as_ref().expect("a state"),
        &SourceVerificationState::VerifiedBytes
    );
}

/// A missing payload and a directory at the payload path fail
/// closed with path-carrying findings (TEST-154).
#[test]
fn missing_payload_and_directory_targets_fail_closed() {
    let digest = digest_of(PAYLOAD_BYTES);

    // The payload root itself is absent.
    let corpus = corpus_of(
        vec![vendored_revision(SRC_1, DOC_1, &digest, VENDORED_PATH)],
        &[],
    );
    let results = verify_effective_sources(corpus.root(), &corpus.graph, &corpus.lock_bytes)
        .expect("prerequisites pass");
    assert!(
        matches!(
            &results[0].outcome,
            Err(SourcePayloadError::MissingPayload {
                source_uid,
                document_key,
                path,
            }) if source_uid == SRC_1
                && document_key == DOC_1
                && path.ends_with(VENDORED_PATH)
        ),
        "a missing payload root fails as MissingPayload, got: {:?}",
        results[0].outcome
    );

    // The root exists but the payload file does not.
    let corpus = corpus_of(
        vec![vendored_revision(SRC_1, DOC_1, &digest, VENDORED_PATH)],
        &[("sources/doc-2/other.pdf", b"unrelated\n")],
    );
    let results = verify_effective_sources(corpus.root(), &corpus.graph, &corpus.lock_bytes)
        .expect("prerequisites pass");
    assert!(
        matches!(
            &results[0].outcome,
            Err(SourcePayloadError::MissingPayload { .. })
        ),
        "a missing payload file fails as MissingPayload, got: {:?}",
        results[0].outcome
    );

    // A regular file sits mid-path where a directory must be.
    let corpus = corpus_of(
        vec![vendored_revision(SRC_1, DOC_1, &digest, VENDORED_PATH)],
        &[("sources/doc-1", b"not a directory\n")],
    );
    let results = verify_effective_sources(corpus.root(), &corpus.graph, &corpus.lock_bytes)
        .expect("prerequisites pass");
    assert!(
        matches!(
            &results[0].outcome,
            Err(SourcePayloadError::MissingPayload { .. })
        ),
        "a non-directory mid-path fails as MissingPayload, got: {:?}",
        results[0].outcome
    );

    // A directory sits at the payload path.
    let corpus = corpus_of(
        vec![vendored_revision(SRC_1, DOC_1, &digest, VENDORED_PATH)],
        &[],
    );
    std::fs::create_dir_all(corpus.root().join(VENDORED_PATH)).unwrap();
    let results = verify_effective_sources(corpus.root(), &corpus.graph, &corpus.lock_bytes)
        .expect("prerequisites pass");
    assert!(
        matches!(
            &results[0].outcome,
            Err(SourcePayloadError::NotAFile {
                source_uid,
                document_key,
                path,
            }) if source_uid == SRC_1
                && document_key == DOC_1
                && path.ends_with(VENDORED_PATH)
        ),
        "a directory target fails as NotAFile, got: {:?}",
        results[0].outcome
    );
}

/// Altered bytes fail as a digest mismatch carrying the declared
/// and the actual digests (TEST-154).
#[test]
fn altered_bytes_report_expected_and_actual_digests() {
    let declared = digest_of(PAYLOAD_BYTES);
    let altered = b"altered payload bytes\n";
    let corpus = corpus_of(
        vec![vendored_revision(SRC_1, DOC_1, &declared, VENDORED_PATH)],
        &[(VENDORED_PATH, altered)],
    );
    let results = verify_effective_sources(corpus.root(), &corpus.graph, &corpus.lock_bytes)
        .expect("prerequisites pass");
    assert!(
        matches!(
            &results[0].outcome,
            Err(SourcePayloadError::DigestMismatch(detail))
                if detail.source_uid == SRC_1
                && detail.document_key == DOC_1
                && detail.path.ends_with(VENDORED_PATH)
                && detail.expected.as_str() == declared
                && detail.actual.as_str() == digest_of(altered)
        ),
        "altered bytes fail as DigestMismatch with both digests, got: {:?}",
        results[0].outcome
    );
}

/// Absolute, parent-component, and non-`sources/` wire paths fail
/// as path escapes. Record loading rejects the first two lexically
/// (LLR-125); the verifier re-checks as defense in depth because a
/// programmatically built graph bypasses record loading (TEST-154).
#[test]
fn unsafe_wire_paths_are_rejected_as_escapes() {
    let corpus = corpus_of(vec![hash_only_revision(SRC_2, DOC_2, &"b".repeat(64))], &[]);
    for wire_path in ["/etc/passwd", "sources/../escape.pdf", "payloads/doc-1.pdf"] {
        let err = resolve_vendored_path(corpus.root(), DOC_1, SRC_1, wire_path)
            .expect_err("an unsafe wire path must fail");
        assert!(
            matches!(err, SourcePayloadError::PathEscape { .. }),
            "wire path {wire_path:?} must fail as PathEscape, got: {err:?}"
        );
    }

    // Through the batch: a programmatically built node carrying an
    // absolute vendored path inserts and locks fine (the lexical
    // gate lives in record loading), and verification still refuses
    // it — per head, without aborting the batch.
    let corpus = corpus_of(
        vec![
            vendored_revision(SRC_1, DOC_1, &digest_of(PAYLOAD_BYTES), "/etc/passwd"),
            hash_only_revision(SRC_2, DOC_2, &"b".repeat(64)),
        ],
        &[],
    );
    let results = verify_effective_sources(corpus.root(), &corpus.graph, &corpus.lock_bytes)
        .expect("prerequisites pass");
    assert_eq!(results.len(), 2);
    assert!(
        matches!(
            &results[0].outcome,
            Err(SourcePayloadError::PathEscape {
                source_uid,
                document_key,
                ..
            }) if source_uid == SRC_1 && document_key == DOC_1
        ),
        "an absolute vendored path fails per head, got: {:?}",
        results[0].outcome
    );
    assert_eq!(
        results[1].outcome.as_ref().expect("a state"),
        &SourceVerificationState::DigestDeclared,
        "the path finding must not hide the later head"
    );
}

/// A symlink payload root and symlinked path components fail
/// closed; the final component being a symlink fails the same way
/// (TEST-154).
#[test]
fn symlink_root_and_components_fail_closed() {
    #[cfg(unix)]
    {
        let digest = digest_of(PAYLOAD_BYTES);

        // The payload root itself is a symlink.
        let corpus = corpus_of(
            vec![vendored_revision(SRC_1, DOC_1, &digest, VENDORED_PATH)],
            &[],
        );
        let real = corpus.root().join("real");
        std::fs::create_dir_all(real.join("doc-1")).unwrap();
        std::fs::write(real.join("doc-1/rev-c.pdf"), PAYLOAD_BYTES).unwrap();
        std::os::unix::fs::symlink(&real, corpus.root().join("sources")).unwrap();
        let err = resolve_vendored_path(corpus.root(), DOC_1, SRC_1, VENDORED_PATH)
            .expect_err("a symlink payload root must fail");
        assert!(
            matches!(
                err,
                SourcePayloadError::SymlinkRoot { ref root }
                    if *root == corpus.root().join("sources")
            ),
            "a symlink payload root fails as SymlinkRoot, got: {err:?}"
        );

        // An intermediate component is a symlink.
        let corpus = corpus_of(
            vec![vendored_revision(SRC_1, DOC_1, &digest, VENDORED_PATH)],
            &[],
        );
        let real = corpus.root().join("real");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::write(real.join("rev-c.pdf"), PAYLOAD_BYTES).unwrap();
        std::fs::create_dir(corpus.root().join("sources")).unwrap();
        std::os::unix::fs::symlink(&real, corpus.root().join("sources/doc-1")).unwrap();
        let err = resolve_vendored_path(corpus.root(), DOC_1, SRC_1, VENDORED_PATH)
            .expect_err("a symlinked component must fail");
        assert!(
            matches!(
                err,
                SourcePayloadError::SymlinkComponent { ref component, .. }
                    if *component == corpus.root().join("sources/doc-1")
            ),
            "a symlinked component fails as SymlinkComponent, got: {err:?}"
        );

        // The final component is a symlink to a real file with
        // matching bytes — still refused.
        let corpus = corpus_of(
            vec![vendored_revision(SRC_1, DOC_1, &digest, VENDORED_PATH)],
            &[],
        );
        let real_file = corpus.root().join("real.pdf");
        std::fs::write(&real_file, PAYLOAD_BYTES).unwrap();
        std::fs::create_dir_all(corpus.root().join("sources/doc-1")).unwrap();
        std::os::unix::fs::symlink(&real_file, corpus.root().join(VENDORED_PATH)).unwrap();
        let err = resolve_vendored_path(corpus.root(), DOC_1, SRC_1, VENDORED_PATH)
            .expect_err("a symlinked payload file must fail");
        assert!(
            matches!(err, SourcePayloadError::SymlinkComponent { .. }),
            "a symlinked payload file fails as SymlinkComponent, got: {err:?}"
        );
    }
    // Off Unix, symlink creation is platform-privileged; the
    // `symlink_metadata` checks are exercised by the Unix CI hosts
    // and this test degrades to a no-op elsewhere.
}

/// A lock entry disagreeing with the record fails as a typed
/// lock/record disagreement naming the field. The global graph-lock
/// equality gate makes this unreachable through the batch, so the
/// per-head check is exercised directly — defense in depth that
/// degrades to a typed finding, never a wrong `VerifiedBytes`
/// (TEST-154).
#[test]
fn lock_record_disagreement_is_typed() {
    let digest = digest_of(PAYLOAD_BYTES);
    let revision =
        super::fixtures::revision_node(SRC_1, DOC_1, vendored_material(&digest, VENDORED_PATH));
    let record_digest = SourceContentDigest::from_hex(&digest).unwrap();
    let corpus = corpus_of(
        vec![vendored_revision(SRC_1, DOC_1, &digest, VENDORED_PATH)],
        &[(VENDORED_PATH, PAYLOAD_BYTES)],
    );
    let agreeing = SourceLockEntry {
        document_key: DOC_1.to_string(),
        source_uid: SRC_1.to_string(),
        availability: LockAvailability::Available,
        sha256: Some(record_digest.clone()),
        capture_mode: Some(LockCaptureMode::Vendored),
        external_control: None,
    };
    let verified = verify_vendored_head(
        corpus.root(),
        DOC_1,
        &revision,
        VENDORED_PATH,
        &record_digest,
        &agreeing,
    );
    assert!(
        matches!(verified, Ok(SourceVerificationState::VerifiedBytes)),
        "an agreeing lock entry verifies, got: {verified:?}"
    );

    let mut wrong_digest = agreeing.clone();
    wrong_digest.sha256 = Some(SourceContentDigest::from_hex(&"f".repeat(64)).unwrap());
    let err = verify_vendored_head(
        corpus.root(),
        DOC_1,
        &revision,
        VENDORED_PATH,
        &record_digest,
        &wrong_digest,
    )
    .expect_err("a digest disagreement must fail");
    assert!(
        matches!(
            err,
            SourcePayloadError::LockDisagreement {
                ref source_uid,
                ref document_key,
                field,
            } if source_uid == SRC_1 && document_key == DOC_1 && field == "digest"
        ),
        "got: {err:?}"
    );

    let mut wrong_mode = agreeing.clone();
    wrong_mode.capture_mode = Some(LockCaptureMode::HashOnly);
    let err = verify_vendored_head(
        corpus.root(),
        DOC_1,
        &revision,
        VENDORED_PATH,
        &record_digest,
        &wrong_mode,
    )
    .expect_err("a capture-mode disagreement must fail");
    assert!(
        matches!(
            err,
            SourcePayloadError::LockDisagreement { field, .. } if field == "capture_mode"
        ),
        "got: {err:?}"
    );

    let unavailable = SourceLockEntry {
        availability: LockAvailability::Unavailable,
        sha256: None,
        capture_mode: None,
        ..agreeing
    };
    let err = verify_vendored_head(
        corpus.root(),
        DOC_1,
        &revision,
        VENDORED_PATH,
        &record_digest,
        &unavailable,
    )
    .expect_err("an availability disagreement must fail");
    assert!(
        matches!(
            err,
            SourcePayloadError::LockDisagreement { field, .. } if field == "availability"
        ),
        "got: {err:?}"
    );
}
