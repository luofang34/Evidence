//! Tests for the optional `supersedes` record field: its edge
//! projection and its fail-closed uid validation (TEST-144).

use super::SOURCE_UID_PREFIX;
use super::tests_support::*;
use crate::corpus::{EdgeKind, SourceError};

/// A record's optional `supersedes` value projects into exactly one
/// owned `Supersedes` edge on the node; an absent value leaves the
/// edge set empty (TEST-144).
#[test]
fn supersedes_field_projects_one_owned_edge() {
    let prior = vendored(SRC_1, "SRC-1");
    let mut newer = vendored(SRC_2, "SRC-2");
    newer.supersedes = Some(SRC_1.to_string());
    let graph = load_source_content(&source_file(&[prior, newer])).expect("a linked pair loads");

    let prior_node = expect_source(&graph, SRC_1);
    assert!(
        prior_node.edges.is_empty(),
        "an absent supersedes leaves the edge set empty, got: {:?}",
        prior_node.edges
    );
    let newer_node = expect_source(&graph, SRC_2);
    assert_eq!(
        newer_node.edges,
        vec![(EdgeKind::Supersedes, SRC_1.to_string())],
        "a present supersedes projects into one owned edge from the newer revision"
    );
}

/// A `supersedes` value with a wrong prefix or a non-UUIDv4 suffix
/// fails closed through the shared native-uid check (TEST-144).
#[test]
fn malformed_supersedes_uid_fails_closed() {
    for (case, target, matches_prefix) in [
        (
            "wrong prefix",
            "req_00000000-0000-4000-8000-00000000000a",
            true,
        ),
        ("not a uuid", "src_not-a-uuid", false),
        ("uuid v1", "src_00000000-0000-1000-8000-00000000000a", false),
    ] {
        let mut spec = vendored(SRC_1, "SRC-1");
        spec.supersedes = Some(target.to_string());
        let err = expect_load_err(load_source_content(&source_file(&[spec])), case);
        let matched = if matches_prefix {
            matches!(
                err,
                SourceError::NativeUidPrefix { ref uid, expected }
                    if uid == target && expected == SOURCE_UID_PREFIX
            )
        } else {
            matches!(err, SourceError::NativeUidUuidV4 { ref uid } if uid == target)
        };
        assert!(
            matched,
            "{case} supersedes must fail with its typed uid variant, got: {err:?}"
        );
    }
}
