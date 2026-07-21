//! Round-trip, fail-closed, schema-negative, and concurrency tests
//! for the append-only proposal store (TEST-139).

use std::collections::BTreeSet;
use std::path::PathBuf;

use super::tests_support::*;
use super::{
    PROPOSAL_UID_PREFIX, ProposalAction, ProposalError, ProposalStore, SUPPORTED_PROPOSAL_SCHEMA,
    write_exclusive_blocking,
};

/// Create round-trip (TEST-139): file at the returned path,
/// basename `<uid>.toml`, strict read-back matches the submitted
/// record, and the returned digest binds the exact file bytes.
#[test]
fn create_proposal_round_trips_through_strict_schema() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir);
    let outcome = store
        .append_create_candidate_blocking(SUBMITTER, content("new candidate"))
        .expect("append succeeds");

    assert!(outcome.proposal_uid.starts_with(PROPOSAL_UID_PREFIX));
    let candidate_uid = outcome
        .candidate_uid
        .as_deref()
        .expect("create mints a candidate uid");
    assert!(candidate_uid.starts_with("req_"));
    let root = dir.path().canonicalize().expect("canonical root");
    assert_eq!(
        outcome.path,
        root.join(format!("{}.toml", outcome.proposal_uid))
    );
    let bytes = std::fs::read(&outcome.path).expect("file exists at returned path");
    assert_eq!(
        outcome.content_digest.as_str(),
        crate::hash::sha256(&bytes),
        "the returned digest binds the exact bytes written"
    );

    let read = ProposalStore::read_proposal_blocking(&outcome.path).expect("strict read-back");
    assert_eq!(read.schema_version, SUPPORTED_PROPOSAL_SCHEMA);
    assert_eq!(read.proposal.uid, outcome.proposal_uid);
    assert_eq!(read.proposal.submitter, SUBMITTER);
    assert!(
        chrono::DateTime::parse_from_rfc3339(&read.proposal.submitted_at).is_ok(),
        "submitted_at is a valid RFC 3339 timestamp"
    );
    let ProposalAction::CreateCandidate {
        candidate_uid: read_uid,
        content: read_content,
    } = read.proposal.action
    else {
        panic!("create action round-trips");
    };
    assert_eq!(read_uid, candidate_uid);
    assert_eq!(read_content, content("new candidate"));
}

/// Revise round-trip against a candidate fixture graph (TEST-139).
#[test]
fn revise_proposal_round_trips_against_candidate() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir);
    let graph = candidate_graph();
    let expected = current_digest(&graph, REQ_A);
    let outcome = revise(&store, &graph, REQ_A, expected.clone()).expect("append succeeds");
    assert!(
        outcome.candidate_uid.is_none(),
        "a revision mints no candidate uid"
    );

    let read = ProposalStore::read_proposal_blocking(&outcome.path).expect("strict read-back");
    let ProposalAction::ReviseCandidate {
        base_uid,
        expected_current_digest,
        content: read_content,
    } = read.proposal.action
    else {
        panic!("revise action round-trips");
    };
    assert_eq!(base_uid, REQ_A);
    assert_eq!(expected_current_digest, expected);
    assert_eq!(read_content, content("replacement"));
}

/// Uids are store-minted: identical inputs still yield distinct
/// proposal uids, basenames, and candidate uids (TEST-139).
#[test]
fn uids_and_timestamps_are_store_minted() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir);
    let first = store
        .append_create_candidate_blocking(SUBMITTER, content("same"))
        .expect("first append");
    let second = store
        .append_create_candidate_blocking(SUBMITTER, content("same"))
        .expect("second append");
    assert_ne!(first.proposal_uid, second.proposal_uid);
    assert_ne!(first.path, second.path);
    assert_ne!(first.candidate_uid, second.candidate_uid);
    assert!(
        first.path.exists() && second.path.exists(),
        "both proposals coexist"
    );
}

/// Appends never collide, and exclusive creation refuses to
/// overwrite a pre-existing file at a would-be path (TEST-139).
#[test]
fn appends_never_overwrite_and_preexisting_path_fails_closed() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir);
    let first = store
        .append_create_candidate_blocking(SUBMITTER, content("one"))
        .expect("first append");
    let second = store
        .append_create_candidate_blocking(SUBMITTER, content("two"))
        .expect("second append");
    assert_ne!(first.path, second.path);

    let root = dir.path().canonicalize().expect("canonical root");
    let occupied = root.join("prop_00000000-0000-4000-8000-0000000000ab.toml");
    write_exclusive_blocking(&occupied, b"first").expect("first write creates");
    let err = write_exclusive_blocking(&occupied, b"second")
        .expect_err("a pre-existing path must fail closed");
    assert!(
        matches!(err, ProposalError::ProposalExists { .. }),
        "exclusive creation reports the collision, got: {err:?}"
    );
    assert_eq!(
        std::fs::read(&occupied).expect("read occupied"),
        b"first",
        "the existing file is never overwritten"
    );
}

/// Missing, non-directory, and symlink roots all fail closed at
/// construction (TEST-139).
#[test]
fn missing_root_fails_closed() {
    let dir = tempfile::tempdir().expect("tempdir");
    let err = ProposalStore::new(&dir.path().join("absent")).expect_err("missing root fails");
    assert!(
        matches!(err, ProposalError::ProposalRootMissing { .. }),
        "got: {err:?}"
    );
}

#[test]
fn non_directory_root_fails_closed() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("file.toml");
    std::fs::write(&file, "x").expect("write file");
    let err = ProposalStore::new(&file).expect_err("file root fails");
    assert!(
        matches!(err, ProposalError::ProposalRootNotADirectory { .. }),
        "got: {err:?}"
    );
}

#[test]
fn symlink_root_fails_closed() {
    let dir = tempfile::tempdir().expect("tempdir");
    let real = dir.path().join("real");
    std::fs::create_dir(&real).expect("mkdir");
    #[cfg(unix)]
    {
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&real, &link).expect("symlink");
        let err = ProposalStore::new(&link).expect_err("symlink root fails");
        assert!(
            matches!(err, ProposalError::ProposalRootSymlink { .. }),
            "got: {err:?}"
        );
    }
    // Off Unix, symlink creation is platform-privileged; the
    // `symlink_metadata` check is exercised by the Unix CI hosts
    // and this test degrades to a no-op elsewhere.
}

/// A truncated or incomplete proposal fails closed on read-back
/// with path context (TEST-139).
#[test]
fn partial_write_read_back_fails_closed() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir);
    let outcome = store
        .append_create_candidate_blocking(SUBMITTER, content("full"))
        .expect("append succeeds");
    let bytes = std::fs::read(&outcome.path).expect("read full proposal");

    let truncated = dir.path().join("truncated.toml");
    std::fs::write(&truncated, &bytes[..40]).expect("write truncated");
    let err = ProposalStore::read_proposal_blocking(&truncated)
        .expect_err("a truncated proposal must fail closed");
    assert!(
        matches!(err, ProposalError::ProposalParse { ref path, .. } if *path == truncated),
        "the typed error names the path, got: {err:?}"
    );

    // A prefix that is valid TOML but lacks the record fails too.
    let err = read_err(&dir, "incomplete.toml", "schema_version = 1\n");
    assert!(
        matches!(err, ProposalError::ProposalParse { .. }),
        "a missing record fails closed, got: {err:?}"
    );
    let err = ProposalStore::read_proposal_blocking(&dir.path().join("absent.toml"))
        .expect_err("a missing file must fail closed");
    assert!(
        matches!(err, ProposalError::ProposalRead { .. }),
        "got: {err:?}"
    );
}

/// Newer schema versions and malformed record fields fail closed
/// with typed context (TEST-139).
#[test]
fn read_back_refuses_newer_schema_and_bad_fields() {
    let dir = tempfile::tempdir().expect("tempdir");
    let err = read_err(
        &dir,
        "newer.toml",
        &doc(&create_action_block()).replace("schema_version = 1", "schema_version = 2"),
    );
    assert!(
        matches!(
            err,
            ProposalError::ProposalSchema {
                found: 2,
                supported: 1,
                ..
            }
        ),
        "got: {err:?}"
    );

    let cases: [(&str, String, &str); 5] = [
        (
            "bad-uid.toml",
            doc(&create_action_block()).replace(
                "prop_00000000-0000-4000-8000-0000000000aa",
                "prop_not-a-uuid",
            ),
            "NativeUidUuidV4",
        ),
        (
            "wrong-prefix.toml",
            doc(&create_action_block()).replace("prop_00000000-0000-4000-8000-0000000000aa", REQ_A),
            "NativeUidPrefix",
        ),
        (
            "bad-time.toml",
            doc(&create_action_block()).replace("2026-07-20T12:00:00Z", "yesterday"),
            "ProposalTimestamp",
        ),
        (
            "empty-submitter.toml",
            doc(&create_action_block()).replace(SUBMITTER, "   "),
            "ProposalSubmitter",
        ),
        (
            "bad-candidate.toml",
            doc(&create_action_block().replace(REQ_A, "req_nope")),
            "NativeUidUuidV4",
        ),
    ];
    for (name, text, expected) in cases {
        let err = read_err(&dir, name, &text);
        let variant = format!("{err:?}");
        assert!(
            variant.starts_with(expected),
            "{name}: expected {expected}, got: {variant}"
        );
    }

    // A revise record whose base uid carries the wrong prefix.
    let revise_block = format!(
        "[proposal.action]\naction = \"revise_candidate\"\n\
         base_uid = \"prop_00000000-0000-4000-8000-0000000000aa\"\n\
         expected_current_digest = \"{DIGEST}\"\n\n\
         [proposal.action.content]\ntitle = \"t\"\nlayer = \"hlr\"\n"
    );
    let err = read_err(&dir, "bad-base.toml", &doc(&revise_block));
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

/// Approval, rejection, deletion, review/source/baseline/file
/// mutations, self-declared actor fields, and unknown fields are
/// all unrepresentable: they fail closed at parse (TEST-139).
#[test]
fn schema_negative_actions_fail_closed() {
    let dir = tempfile::tempdir().expect("tempdir");
    let actions = [
        "approve",
        "reject",
        "delete_requirement",
        "mutate_review",
        "write_baseline",
        "write_file",
    ];
    for (index, action) in actions.iter().enumerate() {
        let block = format!("[proposal.action]\naction = \"{action}\"\n");
        let err = read_err(&dir, &format!("neg-{index}.toml"), &doc(&block));
        assert!(
            matches!(err, ProposalError::ProposalParse { .. }),
            "action {action:?} must be unrepresentable, got: {err:?}"
        );
    }

    let unknown_top = doc(&create_action_block()).replace(
        "schema_version = 1",
        "schema_version = 1\nfrobnicate = true",
    );
    let err = read_err(&dir, "unknown-top.toml", &unknown_top);
    assert!(
        matches!(err, ProposalError::ProposalParse { .. }),
        "an unknown top-level field must fail, got: {err:?}"
    );

    let actor =
        doc(&create_action_block()).replace("submitter =", "actor = \"human\"\nsubmitter =");
    let err = read_err(&dir, "actor.toml", &actor);
    assert!(
        matches!(err, ProposalError::ProposalParse { .. }),
        "a self-declared actor field must fail, got: {err:?}"
    );
}

/// Concurrent submissions cannot overwrite one another: every
/// append succeeds, all basenames are distinct, and every file
/// reads back (TEST-139).
#[test]
fn concurrent_appends_never_overwrite() {
    const THREADS: usize = 4;
    const PER_THREAD: usize = 8;
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir);
    let outcomes = std::sync::Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..THREADS {
            scope.spawn(|| {
                for _ in 0..PER_THREAD {
                    let outcome = store
                        .append_create_candidate_blocking(SUBMITTER, content("concurrent"))
                        .expect("append succeeds");
                    outcomes.lock().expect("lock").push(outcome);
                }
            });
        }
    });
    let outcomes = outcomes.into_inner().expect("lock");
    assert_eq!(outcomes.len(), THREADS * PER_THREAD);
    let paths: BTreeSet<&PathBuf> = outcomes.iter().map(|outcome| &outcome.path).collect();
    assert_eq!(paths.len(), outcomes.len(), "every basename is distinct");
    let uids: BTreeSet<&String> = outcomes
        .iter()
        .map(|outcome| &outcome.proposal_uid)
        .collect();
    assert_eq!(uids.len(), outcomes.len());
    let candidates: BTreeSet<&String> = outcomes
        .iter()
        .filter_map(|outcome| outcome.candidate_uid.as_ref())
        .collect();
    assert_eq!(candidates.len(), outcomes.len());
    for outcome in &outcomes {
        let read = ProposalStore::read_proposal_blocking(&outcome.path).expect("reads back");
        assert_eq!(read.proposal.uid, outcome.proposal_uid);
        assert_eq!(
            outcome.content_digest.as_str(),
            crate::hash::sha256(&std::fs::read(&outcome.path).expect("bytes")),
            "no file was overwritten by a concurrent append"
        );
    }
}
