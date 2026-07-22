//! Tests for the strict source-graph record schema (TEST-168) and
//! for uid prefix validation and identity separation (TEST-169).

use super::error::SourceGraphError;
use super::locator::SourceLocator;
use super::records::{self, SNODE_UID_PREFIX, SUPPORTED_SOURCE_GRAPH_SCHEMA};
use super::{SourceNode, SourceNodeKind};
use crate::corpus::graph::CorpusGraph;
use crate::corpus::source::SOURCE_UID_PREFIX;

const REV_A: &str = "src_00000000-0000-4000-8000-0000000000a1";
const NODE_A: &str = "snode_00000000-0000-4000-8000-0000000000b1";
const NODE_B: &str = "snode_00000000-0000-4000-8000-0000000000b2";
const NODE_C: &str = "snode_00000000-0000-4000-8000-0000000000b3";
const DIGEST_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const DIGEST_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const DIGEST_C: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

fn valid_file() -> String {
    format!(
        r#"schema_version = 1

[[nodes]]
uid = "{NODE_A}"
source_revision_uid = "{REV_A}"
kind = "section"
ordinal = 0
label = "1 Introduction"
canonical_text = ""
content_sha256 = "{DIGEST_A}"
fingerprint = "{DIGEST_B}"

[nodes.locator]
format = "markdown"
path = "docs/spec.md"
anchor = "sec-1"
heading_path = ["Specification", "1 Introduction"]
byte_range = [0, 120]

[[nodes]]
uid = "{NODE_B}"
source_revision_uid = "{REV_A}"
parent_uid = "{NODE_A}"
kind = "paragraph"
ordinal = 0
canonical_text = "First prose."
content_sha256 = "{DIGEST_B}"
fingerprint = "{DIGEST_C}"

[nodes.locator]
format = "markdown"
path = "docs/spec.md"
git_blob = "0123456789abcdef0123456789abcdef01234567"
heading_path = ["Specification", "1 Introduction"]
byte_range = [121, 132]

[[nodes]]
uid = "{NODE_C}"
source_revision_uid = "{REV_A}"
kind = "code_block"
ordinal = 1
canonical_text = "fn main() {{}}\n"
content_sha256 = "{DIGEST_C}"
fingerprint = "{DIGEST_A}"

[nodes.locator]
format = "markdown"
path = "docs/spec.md"
byte_range = [133, 150]
"#
    )
}

fn load_file(content: &str) -> Result<CorpusGraph, SourceGraphError> {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("graphs.toml");
    std::fs::write(&path, content).expect("write records");
    let mut graph = CorpusGraph::new();
    records::load_source_graphs_into(&path, &mut graph)?;
    Ok(graph)
}

/// A valid file projects every field into typed nodes, in uid
/// order regardless of record order (TEST-168).
#[test]
fn strict_file_round_trip_preserves_every_field() {
    let graph = load_file(&valid_file()).expect("valid file loads");
    let source_graph = graph
        .source_graph(REV_A)
        .expect("revision graph materialized");
    assert_eq!(source_graph.len(), 3);
    assert_eq!(source_graph.revision_uid(), Some(REV_A));

    let section = source_graph.get(NODE_A).expect("section node");
    let expected = SourceNode {
        uid: NODE_A.to_string(),
        source_revision_uid: REV_A.to_string(),
        parent_uid: None,
        kind: SourceNodeKind::Section,
        ordinal: 0,
        label: Some("1 Introduction".to_string()),
        canonical_text: String::new(),
        content_sha256: crate::corpus::StructuralContentDigest::from_hex(DIGEST_A).expect("digest"),
        fingerprint: crate::corpus::StructuralContentDigest::from_hex(DIGEST_B).expect("digest"),
        locator: section.locator.clone(),
    };
    assert_eq!(*section, expected);
    let SourceLocator::Markdown {
        path,
        git_blob,
        anchor,
        heading_path,
        byte_range,
    } = &section.locator
    else {
        panic!("section must carry a markdown locator");
    };
    assert_eq!(path.as_str(), "docs/spec.md");
    assert_eq!(git_blob, &None);
    assert_eq!(anchor.as_deref(), Some("sec-1"));
    assert_eq!(
        heading_path,
        &vec!["Specification".to_string(), "1 Introduction".to_string()]
    );
    assert_eq!(*byte_range, (0, 120));

    let paragraph = source_graph.get(NODE_B).expect("paragraph node");
    assert_eq!(paragraph.parent_uid.as_deref(), Some(NODE_A));
    assert_eq!(paragraph.kind, SourceNodeKind::Paragraph);
    assert_eq!(paragraph.label, None);
    let SourceLocator::Markdown {
        git_blob: Some(blob),
        ..
    } = &paragraph.locator
    else {
        panic!("paragraph must carry its git blob");
    };
    assert_eq!(blob, "0123456789abcdef0123456789abcdef01234567");

    // Iteration is uid-ordered, not record-ordered.
    let uids: Vec<&str> = source_graph.nodes().map(|node| node.uid.as_str()).collect();
    assert_eq!(uids, [NODE_A, NODE_B, NODE_C]);
}

/// Unknown fields at every level, unknown enum tags, and a newer
/// schema all fail closed (TEST-168).
#[test]
fn unknown_fields_and_newer_schema_fail_closed() {
    let base = valid_file();
    let cases: [(&str, String); 6] = [
        (
            "unknown file field",
            base.replace("schema_version = 1", "schema_version = 1\nbogus = 1"),
        ),
        (
            "unknown record field",
            base.replace("ordinal = 0\nlabel", "ordinal = 0\nbogus = 1\nlabel"),
        ),
        (
            "unknown locator field",
            base.replace(
                "byte_range = [0, 120]",
                "byte_range = [0, 120]\ndom_path = [0]",
            ),
        ),
        (
            "unknown kind tag",
            base.replace("kind = \"section\"", "kind = \"chapter\""),
        ),
        (
            "unknown format tag",
            base.replace(
                "format = \"markdown\"\npath = \"docs/spec.md\"\nanchor",
                "format = \"epub\"\npath = \"docs/spec.md\"\nanchor",
            ),
        ),
        ("newer schema", "schema_version = 999\n".to_string()),
    ];
    for (name, content) in cases {
        let err = load_file(&content).expect_err("degenerate input must fail closed");
        if name == "newer schema" {
            assert!(
                matches!(
                    err,
                    SourceGraphError::RecordSchemaTooNew {
                        found: 999,
                        supported: SUPPORTED_SOURCE_GRAPH_SCHEMA,
                        ..
                    }
                ),
                "{name}: expected RecordSchemaTooNew, got: {err:?}"
            );
        } else {
            assert!(
                matches!(err, SourceGraphError::RecordParse { .. }),
                "{name}: expected RecordParse, got: {err:?}"
            );
        }
    }
}

/// Malformed digests, a blank label, an invalid locator field, and
/// a wrongly-prefixed parent uid each fail closed with typed
/// context (TEST-168).
#[test]
fn malformed_digests_and_field_values_fail_closed() {
    let base = valid_file();
    let err = load_file(&base.replace(DIGEST_A, &DIGEST_A.to_uppercase()))
        .expect_err("uppercase digest must fail");
    assert!(
        matches!(err, SourceGraphError::RecordParse { .. }),
        "a malformed digest fails at deserialization, got: {err:?}"
    );

    let err = load_file(&base.replace(DIGEST_B, "bbbb")).expect_err("short digest must fail");
    assert!(
        matches!(err, SourceGraphError::RecordParse { .. }),
        "a short digest fails at deserialization, got: {err:?}"
    );

    let err = load_file(&base.replace("label = \"1 Introduction\"", "label = \"   \""))
        .expect_err("blank label must fail");
    assert!(
        matches!(
            err,
            SourceGraphError::NodeLabel { ref uid, .. } if uid == NODE_A
        ),
        "expected NodeLabel naming the record, got: {err:?}"
    );

    let err = load_file(&base.replace("byte_range = [121, 132]", "byte_range = [132, 121]"))
        .expect_err("reversed byte range must fail");
    assert!(
        matches!(
            err,
            SourceGraphError::InvalidLocatorField {
                field: "byte_range",
                rule: crate::corpus::LocatorRule::ByteRangeReversed,
                ref node_uid,
                ..
            } if node_uid == NODE_B
        ),
        "expected InvalidLocatorField naming byte_range, got: {err:?}"
    );

    let err = load_file(&base.replace(
        &format!("parent_uid = \"{NODE_A}\""),
        &format!("parent_uid = \"{REV_A}\""),
    ))
    .expect_err("src_-prefixed parent must fail");
    assert!(
        matches!(
            err,
            SourceGraphError::NativeUidPrefix {
                expected: SNODE_UID_PREFIX,
                ..
            }
        ),
        "expected NativeUidPrefix for the parent field, got: {err:?}"
    );
}

/// Every uid field validates its typed prefix and the UUIDv4
/// shape (TEST-169).
#[test]
fn snode_and_src_uids_validate_prefix_and_version() {
    let base = valid_file();

    // Wrong prefixes on each uid-carrying field.
    let err = load_file(&base.replace(
        &format!("uid = \"{NODE_A}\""),
        &format!("uid = \"{REV_A}\""),
    ))
    .expect_err("src_-prefixed node uid must fail");
    assert!(
        matches!(
            err,
            SourceGraphError::NativeUidPrefix {
                expected: SNODE_UID_PREFIX,
                ..
            }
        ),
        "expected NativeUidPrefix for the node uid, got: {err:?}"
    );

    let err = load_file(&base.replace(
        &format!("source_revision_uid = \"{REV_A}\""),
        &format!("source_revision_uid = \"{NODE_A}\""),
    ))
    .expect_err("snode_-prefixed revision uid must fail");
    assert!(
        matches!(
            err,
            SourceGraphError::NativeUidPrefix {
                expected: SOURCE_UID_PREFIX,
                ..
            }
        ),
        "expected NativeUidPrefix for the revision uid, got: {err:?}"
    );

    // A UUID that is not version 4 (version nibble `1`) fails the
    // UUIDv4 gate on every field.
    let v1_node = NODE_A.replace("4000", "1000");
    let err = load_file(&base.replace(NODE_A, &v1_node)).expect_err("UUIDv1 must fail");
    assert!(
        matches!(err, SourceGraphError::NativeUidUuidV4 { .. }),
        "expected NativeUidUuidV4, got: {err:?}"
    );

    let v1_rev = REV_A.replace("4000", "1000");
    let err = load_file(&base.replace(REV_A, &v1_rev)).expect_err("UUIDv1 must fail");
    assert!(
        matches!(err, SourceGraphError::NativeUidUuidV4 { .. }),
        "expected NativeUidUuidV4 for the revision uid, got: {err:?}"
    );

    // The valid file still loads — v4 uids pass every field.
    load_file(&base).expect("UUIDv4 uids load");
}

/// Structural node identity and frozen revision identity are
/// separate typed namespaces: prefixes never cross, uid
/// uniqueness is per revision, and the same `snode_` uid may
/// recur across revisions of one document (TEST-169).
#[test]
fn source_node_identity_stays_separate_from_revision_identity() {
    let base = valid_file();
    let rev_b = "src_00000000-0000-4000-8000-0000000000a2";

    // The same node uid under a second revision loads — identity
    // reuse across revisions is the design, not a collision.
    let second_revision = base.replace(
        &format!("source_revision_uid = \"{REV_A}\""),
        &format!("source_revision_uid = \"{rev_b}\""),
    );
    let combined = format!(
        "schema_version = 1\n\n{}\n\n{}",
        base.trim_start_matches("schema_version = 1\n"),
        second_revision.trim_start_matches("schema_version = 1\n")
    );
    let graph = load_file(&combined).expect("per-revision identity reuse loads");
    assert_eq!(
        graph
            .source_graph(REV_A)
            .expect("first revision graph")
            .len(),
        3
    );
    assert_eq!(
        graph
            .source_graph(rev_b)
            .expect("second revision graph")
            .len(),
        3
    );

    // The same uid twice within one revision collides.
    let duplicate = format!(
        "{base}\n{}",
        &base[base.find("[[nodes]]").expect("nodes")..]
    );
    let err = load_file(&duplicate).expect_err("duplicate uid in one revision must fail");
    assert!(
        matches!(
            err,
            SourceGraphError::DuplicateUid {
                ref revision_uid,
                ref uid,
            } if revision_uid == REV_A && uid == NODE_A
        ),
        "expected DuplicateUid naming the revision and uid, got: {err:?}"
    );
}
