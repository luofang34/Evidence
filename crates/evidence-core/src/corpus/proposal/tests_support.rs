//! Shared fixtures for the proposal-store test modules (TEST-139,
//! TEST-140). No `#[test]` functions live here.

use std::path::Path;

use tempfile::TempDir;

use super::{
    AppendOutcome, CorpusGraph, ProposalError, ProposalStore, ProposedRequirementContent,
    ReviewContentDigest,
};
use crate::corpus::{
    EdgeKind, Node, RequirementLayer, RequirementNode, RequirementReviewContentV1, ReviewDecision,
    ReviewNode, review_content_digest_v1,
};

pub(super) const REQ_A: &str = "req_00000000-0000-4000-8000-00000000000a";
pub(super) const REQ_B: &str = "req_00000000-0000-4000-8000-00000000000b";
pub(super) const REV_1: &str = "rev_00000000-0000-4000-8000-0000000000a1";
pub(super) const DIGEST: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
pub(super) const SUBMITTER: &str = "agent@example.com";

pub(super) fn content(title: &str) -> ProposedRequirementContent {
    ProposedRequirementContent {
        title: title.to_string(),
        layer: RequirementLayer::Hlr,
        description: Some(format!("normative prose of {title}")),
        rationale: None,
        scope: None,
        category: None,
        source: None,
        verification_methods: vec!["test".to_string()],
        derives_from: Vec::new(),
        safety_impact: None,
    }
}

pub(super) fn requirement(uid: &str, description: &str) -> RequirementNode {
    let mut node = RequirementNode::new(
        uid.to_string(),
        "R-A".to_string(),
        format!("title of {uid}"),
        RequirementLayer::Hlr,
        Vec::new(),
    );
    node.description = Some(description.to_string());
    node
}

pub(super) fn review(
    uid: &str,
    requirement_uid: &str,
    digest: &ReviewContentDigest,
    decision: ReviewDecision,
) -> ReviewNode {
    ReviewNode {
        uid: uid.to_string(),
        id: uid.to_string(),
        requirement_uid: requirement_uid.to_string(),
        content_schema: 1,
        reviewed_content_sha256: digest.clone(),
        decision,
        reviewer: "alice@example.com".to_string(),
        reviewed_at: "2026-07-01T10:00:00Z".to_string(),
        rationale: match decision {
            ReviewDecision::Approve => None,
            ReviewDecision::Reject => Some("found wanting".to_string()),
        },
        edges: vec![(EdgeKind::Reviews, requirement_uid.to_string())],
    }
}

pub(super) fn current_digest(graph: &CorpusGraph, uid: &str) -> ReviewContentDigest {
    review_content_digest_v1(
        &graph
            .review_content(uid)
            .expect("requirement projects content"),
    )
}

pub(super) fn digest_of_prose(description: &str) -> ReviewContentDigest {
    review_content_digest_v1(&RequirementReviewContentV1::from_node(&requirement(
        REQ_A,
        description,
    )))
}

pub(super) fn graph_with(req: RequirementNode, reviews: Vec<ReviewNode>) -> CorpusGraph {
    let mut graph = CorpusGraph::new();
    graph
        .insert(Node::Requirement(req))
        .expect("insert requirement");
    for review in reviews {
        graph.insert(Node::Review(review)).expect("insert review");
    }
    graph
}

pub(super) fn candidate_graph() -> CorpusGraph {
    graph_with(requirement(REQ_A, "prose v1"), Vec::new())
}

/// A candidate at the derived layer whose current content carries
/// a normative `safety_impact`.
pub(super) fn derived_candidate_graph() -> CorpusGraph {
    let mut node = requirement(REQ_A, "prose v1");
    node.layer = RequirementLayer::Derived;
    node.safety_impact = Some("high".to_string());
    graph_with(node, Vec::new())
}

pub(super) fn approved_graph() -> CorpusGraph {
    let digest = digest_of_prose("prose v1");
    graph_with(
        requirement(REQ_A, "prose v1"),
        vec![review(REV_1, REQ_A, &digest, ReviewDecision::Approve)],
    )
}

pub(super) fn rejected_graph() -> CorpusGraph {
    let digest = digest_of_prose("prose v1");
    graph_with(
        requirement(REQ_A, "prose v1"),
        vec![review(REV_1, REQ_A, &digest, ReviewDecision::Reject)],
    )
}

pub(super) fn stale_graph() -> CorpusGraph {
    let older = digest_of_prose("prose v0");
    graph_with(
        requirement(REQ_A, "prose v1"),
        vec![review(REV_1, REQ_A, &older, ReviewDecision::Approve)],
    )
}

pub(super) fn open_store(dir: &TempDir) -> ProposalStore {
    ProposalStore::new(dir.path()).expect("store opens")
}

pub(super) fn revise(
    store: &ProposalStore,
    graph: &CorpusGraph,
    uid: &str,
    digest: ReviewContentDigest,
) -> Result<AppendOutcome, ProposalError> {
    store.append_revise_candidate_blocking(graph, uid, digest, SUBMITTER, content("replacement"))
}

pub(super) fn create_action_block() -> String {
    format!(
        "[proposal.action]\naction = \"create_candidate\"\ncandidate_uid = \"{REQ_A}\"\n\n\
         [proposal.action.content]\ntitle = \"t\"\nlayer = \"hlr\"\n"
    )
}

pub(super) fn doc(action_block: &str) -> String {
    format!(
        "schema_version = 1\n\n[proposal]\nuid = \"prop_00000000-0000-4000-8000-0000000000aa\"\n\
         submitter = \"{SUBMITTER}\"\nsubmitted_at = \"2026-07-20T12:00:00Z\"\n\n{action_block}\n"
    )
}

pub(super) fn read_err(dir: &TempDir, name: &str, text: &str) -> ProposalError {
    let path = dir.path().join(name);
    std::fs::write(&path, text).expect("write fixture");
    ProposalStore::read_proposal_blocking(&path).expect_err("must fail closed")
}

/// Number of entries beneath `dir`, excluding the root directory
/// itself: 0 means the directory is empty. Uses `walkdir`, the
/// workspace-sanctioned walker, with links unfollowed.
pub(super) fn entry_count(dir: &TempDir) -> usize {
    walkdir::WalkDir::new(dir.path())
        .follow_links(false)
        .into_iter()
        .count()
        .saturating_sub(1)
}

pub(super) fn write(path: &Path, text: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("mkdir");
    }
    std::fs::write(path, text).expect("write");
}
