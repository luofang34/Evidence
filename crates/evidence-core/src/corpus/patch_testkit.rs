//! Shared fixtures for curated-patch graph tests: a committed
//! patch bound to one source revision over an empty parser graph,
//! plus typed-target review nodes (TEST-189, TEST-190, TEST-191).
//!
//! `#[cfg(test)]`-only; every constructor panics on fixture bugs so
//! a broken fixture fails immediately at setup.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use crate::corpus::digest::{ReviewContentDigest, StructuralContentDigest};
use crate::corpus::graph::{
    CorpusGraph, EdgeKind, Node, ReviewDecision, ReviewNode, ReviewTarget, SourceMaterial,
    SourceRevisionNode,
};
use crate::corpus::source_graph::locator::{SafeRelPath, SourceLocator};
use crate::corpus::source_graph::{SourceGraph, SourceNodeKind};
use crate::corpus::source_patch::digest::{reviewed_content_digest, source_graph_digest};
use crate::corpus::source_patch::records::SourcePatchRecord;
use crate::corpus::source_patch::{InsertedNodeSpec, PatchOperation};

pub(crate) const REVISION: &str = "src_00000000-0000-4000-8000-0000000000a1";
pub(crate) const PATCH_A: &str = "patch_00000000-0000-4000-8000-0000000000b1";
pub(crate) const INSERTED: &str = "snode_00000000-0000-4000-8000-0000000000c1";
pub(crate) const REV_1: &str = "rev_00000000-0000-4000-8000-0000000000a1";
pub(crate) const REV_2: &str = "rev_00000000-0000-4000-8000-0000000000a2";
pub(crate) const REV_3: &str = "rev_00000000-0000-4000-8000-0000000000a3";
pub(crate) const RECIPE_HEX: &str =
    "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
pub(crate) const INPUT_HEX: &str =
    "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
pub(crate) const MEDIA: &str = "text/markdown";

pub(crate) fn review_digest(hex: &str) -> ReviewContentDigest {
    ReviewContentDigest::from_hex(hex).unwrap()
}

pub(crate) fn structural(hex: &str) -> StructuralContentDigest {
    StructuralContentDigest::from_hex(hex).unwrap()
}

/// The one source revision the fixture patches bind: unavailable
/// material, no lineage edges.
pub(crate) fn revision_node() -> Node {
    Node::SourceRevision(SourceRevisionNode {
        uid: REVISION.to_string(),
        id: "DOC-1".to_string(),
        document_key: "doc".to_string(),
        title: "fixture document".to_string(),
        media_type: MEDIA.to_string(),
        canonical_location: "https://example.org/doc/rev-a".to_string(),
        material: SourceMaterial::Unavailable {
            reason: "fixture".to_string(),
        },
        edges: Vec::new(),
    })
}

/// A committed patch over the revision's empty parser graph: one
/// root `insert` operation, which applies cleanly to the empty
/// graph, and a recomputed reviewed-content digest.
pub(crate) fn patch_record(uid: &str, human_id: &str) -> SourcePatchRecord {
    let mut record = SourcePatchRecord {
        uid: uid.to_string(),
        human_id: human_id.to_string(),
        source_revision_uid: REVISION.to_string(),
        recipe_digest: structural(RECIPE_HEX),
        input_digest: structural(INPUT_HEX),
        pre_patch_graph_digest: source_graph_digest(&SourceGraph::new()),
        reviewed_content_digest: structural(&"0".repeat(64)),
        author: "curator@example.com".to_string(),
        rationale: "restore the intended structure".to_string(),
        created_at: "2026-07-01T10:00:00Z".to_string(),
        operations: vec![PatchOperation::Insert {
            ordinal: 0,
            expected_parent_uid: None,
            node: InsertedNodeSpec {
                uid: INSERTED.to_string(),
                kind: SourceNodeKind::Section,
                ordinal: 0,
                label: None,
                canonical_text: "curated section".to_string(),
                locator: SourceLocator::Markdown {
                    path: SafeRelPath::new("docs/doc.md").unwrap(),
                    git_blob: None,
                    anchor: None,
                    heading_path: Vec::new(),
                    byte_range: (0, 10),
                },
            },
        }],
    };
    record.reviewed_content_digest = reviewed_content_digest(&record);
    record
}

/// A review node targeting `target_uid` as a curated patch, over
/// the given reviewed-content digest hex.
pub(crate) fn patch_review(
    uid: &str,
    id: &str,
    patch_uid: &str,
    digest_hex: &str,
    decision: ReviewDecision,
    reviewer: &str,
    supersedes: Option<&str>,
) -> ReviewNode {
    let mut edges = vec![(EdgeKind::Reviews, patch_uid.to_string())];
    if let Some(predecessor) = supersedes {
        edges.push((EdgeKind::Supersedes, predecessor.to_string()));
    }
    ReviewNode {
        uid: uid.to_string(),
        id: id.to_string(),
        target: ReviewTarget::CuratedPatch(patch_uid.to_string()),
        content_schema: 1,
        reviewed_content_sha256: review_digest(digest_hex),
        decision,
        reviewer: reviewer.to_string(),
        reviewed_at: "2026-07-01T10:00:00Z".to_string(),
        rationale: (decision == ReviewDecision::Reject).then(|| "not ready".to_string()),
        edges,
    }
}

/// A graph carrying the revision, the patch, and the reviews.
pub(crate) fn graph_with(patch: SourcePatchRecord, reviews: Vec<ReviewNode>) -> CorpusGraph {
    let mut graph = CorpusGraph::new();
    graph.insert(revision_node()).unwrap();
    graph.insert_source_patch(patch).unwrap();
    for review in reviews {
        graph.insert(Node::Review(review)).unwrap();
    }
    graph
}
