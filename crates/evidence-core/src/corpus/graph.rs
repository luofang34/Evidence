//! Node/edge types and the uid-keyed corpus graph.
//!
//! Nodes live in a `BTreeMap` keyed by uid, and outgoing edges are
//! sorted, so graph equality and derived views are independent of
//! input order (HLR-080). Edges are typed and owned by their source
//! node; invalid endpoints are validation errors.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;

use super::digest::ReviewContentDigest;
use super::error::CorpusError;

mod review_invariants;
mod supersession;

/// Typed edge kinds supported by the corpus graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    /// Requirement → parent requirement it decomposes.
    DerivesFrom,
    /// Test → requirement it verifies.
    Verifies,
    /// Review → requirement whose content it decides on (LLR-115).
    Reviews,
    /// Review → earlier review it corrects (LLR-115).
    Supersedes,
}

/// Requirement decomposition layer within the graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequirementLayer {
    /// Extracted directly from a frozen source document.
    Source,
    /// System requirement.
    Sys,
    /// High-level requirement.
    Hlr,
    /// Low-level requirement.
    Llr,
    /// Derived requirement (no parent by definition).
    Derived,
}

impl RequirementLayer {
    /// The serde `snake_case` wire string for this layer — the form
    /// bound into the canonical review-content encoding (LLR-111).
    pub fn as_str(self) -> &'static str {
        match self {
            RequirementLayer::Source => "source",
            RequirementLayer::Sys => "sys",
            RequirementLayer::Hlr => "hlr",
            RequirementLayer::Llr => "llr",
            RequirementLayer::Derived => "derived",
        }
    }
}

/// Coarse node kind, for kind-filtered queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    /// A requirement at any [`RequirementLayer`].
    Requirement,
    /// A test case.
    Test,
    /// A human review decision record (LLR-115).
    Review,
}

/// A human review decision over a requirement's exact reviewed
/// content (LLR-115).
///
/// The decision is audit data only: computing an effective lifecycle
/// state from decisions is a derived view, and `reviewed_at` never
/// picks a winner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewDecision {
    /// The reviewer approves the reviewed content.
    Approve,
    /// The reviewer rejects the reviewed content; the record
    /// carries a non-empty rationale.
    Reject,
}

/// Trace metadata retained for requirement-derived views.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct RequirementMetadata {
    /// Optional namespace prefix for the human-readable identifier.
    pub(crate) namespace: Option<String>,
    /// Explicit presentation order within the requirement layer.
    pub(crate) sort_key: Option<i64>,
    /// Requirement scope.
    pub(crate) scope: Option<String>,
    /// Requirement category.
    pub(crate) category: Option<String>,
    /// Source reference.
    pub(crate) source: Option<String>,
    /// Implementation modules associated with the requirement.
    pub(crate) modules: Vec<String>,
    /// User-visible surfaces governed by an HLR.
    pub(crate) surfaces: Vec<String>,
    /// Diagnostic codes emitted by an LLR.
    pub(crate) emits: Vec<String>,
    /// Verification methods required by trace policy.
    pub(crate) verification_methods: Vec<String>,
}

/// Trace metadata retained for test-derived views.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct TestMetadata {
    /// Optional namespace prefix for the human-readable identifier.
    pub(crate) namespace: Option<String>,
    /// Explicit presentation order within the test layer.
    pub(crate) sort_key: Option<i64>,
    /// Test category.
    pub(crate) category: Option<String>,
    /// Source reference.
    pub(crate) source: Option<String>,
    /// Primary selector displayed by trace reports.
    pub(crate) primary_selector: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TraceMetadata {
    Requirement(RequirementMetadata),
    Test(TestMetadata),
}

/// A requirement node.
///
/// The content fields past `edges` are the normative statement a
/// review approves (LLR-113) — retained on the node as content, not
/// trace metadata. They play no role in node identity: uniqueness is
/// uid (and human id within a kind), and graph equality already
/// treats layout and edge order as non-semantic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequirementNode {
    /// Permanent identity, unique across all node kinds.
    pub uid: String,
    /// Human-readable identifier (e.g. `HLR-080`).
    pub id: String,
    /// One-line title.
    pub title: String,
    /// Decomposition layer.
    pub layer: RequirementLayer,
    /// Outgoing typed edges `(kind, target uid)`.
    pub edges: Vec<(EdgeKind, String)>,
    /// Normative requirement description (review content).
    pub description: Option<String>,
    /// Normative rationale (review content).
    pub rationale: Option<String>,
    /// Requirement scope (review content).
    pub scope: Option<String>,
    /// Requirement category (review content).
    pub category: Option<String>,
    /// Source reference (review content).
    pub source: Option<String>,
    /// Verification methods, sorted and deduplicated at load
    /// (review content).
    pub verification_methods: Vec<String>,
    /// Safety impact of a derived requirement — normative assurance
    /// content the v1 review-content projection binds (review
    /// content). `None` for non-derived layers, whose source
    /// entries carry no such field.
    pub safety_impact: Option<String>,
}

impl RequirementNode {
    /// A requirement node with no review-content fields populated
    /// yet. Loaders set the content fields directly.
    pub fn new(
        uid: String,
        id: String,
        title: String,
        layer: RequirementLayer,
        edges: Vec<(EdgeKind, String)>,
    ) -> Self {
        Self {
            uid,
            id,
            title,
            layer,
            edges,
            description: None,
            rationale: None,
            scope: None,
            category: None,
            source: None,
            verification_methods: Vec::new(),
            safety_impact: None,
        }
    }
}

/// A test-case node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestNode {
    /// Permanent identity, unique across all node kinds.
    pub uid: String,
    /// Human-readable identifier (e.g. `TEST-120`).
    pub id: String,
    /// One-line title.
    pub title: String,
    /// Test-function selectors, sorted and deduplicated.
    pub selectors: Vec<String>,
    /// Outgoing typed edges `(kind, target uid)`.
    pub edges: Vec<(EdgeKind, String)>,
}

/// A human review decision node (LLR-115).
///
/// One record of a reviewer approving or rejecting one requirement's
/// exact reviewed content. The `reviewer` and `reviewed_at` fields
/// are audit metadata: the reviewer identity is recorded, never
/// accepted as proof that a caller is human, and the timestamp
/// never chooses an effective decision. Edges carry
/// [`EdgeKind::Reviews`] → `requirement_uid` and, for a correcting
/// review, [`EdgeKind::Supersedes`] → the predecessor review uid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewNode {
    /// Permanent identity, unique across all node kinds.
    pub uid: String,
    /// Human-readable identifier (e.g. `REV-001`).
    pub id: String,
    /// Uid of the requirement whose content was reviewed.
    pub requirement_uid: String,
    /// Review-content projection version the digest covers.
    pub content_schema: u32,
    /// Exact digest of the reviewed canonical content.
    pub reviewed_content_sha256: ReviewContentDigest,
    /// The recorded decision.
    pub decision: ReviewDecision,
    /// Organization-stable reviewer identity (audit metadata).
    pub reviewer: String,
    /// RFC 3339 review timestamp (audit metadata only).
    pub reviewed_at: String,
    /// Why the decision was made; required and non-empty for
    /// rejections.
    pub rationale: Option<String>,
    /// Outgoing typed edges `(kind, target uid)`.
    pub edges: Vec<(EdgeKind, String)>,
}

/// A corpus graph node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    /// A requirement at any layer.
    Requirement(RequirementNode),
    /// A test case.
    Test(TestNode),
    /// A human review decision.
    Review(ReviewNode),
}

impl Node {
    /// The node's permanent uid.
    pub fn uid(&self) -> &str {
        match self {
            Node::Requirement(r) => &r.uid,
            Node::Test(t) => &t.uid,
            Node::Review(r) => &r.uid,
        }
    }

    /// The node's coarse kind.
    pub fn kind(&self) -> NodeKind {
        match self {
            Node::Requirement(_) => NodeKind::Requirement,
            Node::Test(_) => NodeKind::Test,
            Node::Review(_) => NodeKind::Review,
        }
    }

    /// The node's human-readable identifier.
    pub fn id(&self) -> &str {
        match self {
            Node::Requirement(r) => &r.id,
            Node::Test(t) => &t.id,
            Node::Review(r) => &r.id,
        }
    }

    /// The node's outgoing typed edges.
    pub fn edges(&self) -> &[(EdgeKind, String)] {
        match self {
            Node::Requirement(r) => &r.edges,
            Node::Test(t) => &t.edges,
            Node::Review(r) => &r.edges,
        }
    }

    fn edges_mut(&mut self) -> &mut Vec<(EdgeKind, String)> {
        match self {
            Node::Requirement(r) => &mut r.edges,
            Node::Test(t) => &mut t.edges,
            Node::Review(r) => &mut r.edges,
        }
    }
}

/// The uid-keyed corpus graph.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CorpusGraph {
    nodes: BTreeMap<String, Node>,
    trace_metadata: BTreeMap<String, TraceMetadata>,
}

impl CorpusGraph {
    /// An empty graph.
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a node after enforcing identity uniqueness and
    /// canonical, duplicate-free edge order.
    pub fn insert(&mut self, mut node: Node) -> Result<(), CorpusError> {
        let uid = node.uid().to_string();
        if self.nodes.contains_key(&uid) {
            return Err(CorpusError::DuplicateUid { uid });
        }
        if let Some(existing) = self
            .nodes
            .values()
            .find(|existing| existing.kind() == node.kind() && existing.id() == node.id())
        {
            return Err(CorpusError::DuplicateHumanId {
                id: node.id().to_string(),
                kind: node.kind(),
                first_uid: existing.uid().to_string(),
                duplicate_uid: uid,
            });
        }
        canonicalize_edges(&mut node)?;
        self.nodes.insert(uid, node);
        Ok(())
    }

    /// Look up a node by uid.
    pub fn get(&self, uid: &str) -> Option<&Node> {
        self.nodes.get(uid)
    }

    /// Number of nodes in the graph.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Whether the graph has no nodes.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Iterate nodes in deterministic (uid) order.
    pub fn nodes(&self) -> impl Iterator<Item = &Node> {
        self.nodes.values()
    }

    /// All review nodes targeting `requirement_uid`, in uid order
    /// (LLR-118). Read-only derived view: supersession and decision
    /// semantics are the lifecycle evaluator's concern, not this
    /// accessor's.
    pub fn reviews_for_requirement(&self, requirement_uid: &str) -> Vec<&ReviewNode> {
        self.nodes
            .values()
            .filter_map(|node| match node {
                Node::Review(review) if review.requirement_uid == requirement_uid => Some(review),
                _ => None,
            })
            .collect()
    }

    /// Uids of every review named as a [`EdgeKind::Supersedes`]
    /// target — the reviews a correcting review has replaced
    /// (LLR-118). Borrows from the graph; `BTreeSet` keeps
    /// membership checks deterministic. A well-formed graph
    /// (`validate`) guarantees every target is a review; an
    /// unvalidated graph may name anything, and the set simply
    /// reports the targets as recorded.
    pub fn superseded_review_uids(&self) -> BTreeSet<&str> {
        self.nodes
            .values()
            .filter_map(|node| match node {
                Node::Review(review) => Some(review),
                _ => None,
            })
            .flat_map(|review| review.edges.iter())
            .filter(|(kind, _)| *kind == EdgeKind::Supersedes)
            .map(|(_, target)| target.as_str())
            .collect()
    }

    pub(crate) fn trace_metadata(&self, uid: &str) -> Option<&TraceMetadata> {
        self.trace_metadata.get(uid)
    }

    pub(super) fn insert_with_trace_metadata(
        &mut self,
        node: Node,
        metadata: TraceMetadata,
    ) -> Result<(), CorpusError> {
        let uid = node.uid().to_string();
        self.insert(node)?;
        self.trace_metadata.insert(uid, metadata);
        Ok(())
    }

    /// Check every edge resolves and obeys its source/target kind
    /// contract, then enforce the per-node review invariants
    /// (exactly one `Reviews` edge agreeing with `requirement_uid`,
    /// supported content schema), then validate review supersession
    /// chains (LLR-115). Review failures surface as
    /// [`CorpusError::Review`] wrapping the typed review error.
    pub fn validate(&self) -> Result<(), CorpusError> {
        for node in self.nodes.values() {
            for (kind, target) in node.edges() {
                let target_node =
                    self.nodes
                        .get(target)
                        .ok_or_else(|| CorpusError::DanglingEdge {
                            from: node.uid().to_string(),
                            to: target.clone(),
                            kind: *kind,
                        })?;
                if !edge_kinds_match(node.kind(), *kind, target_node.kind()) {
                    return Err(CorpusError::InvalidEdgeKinds {
                        from: node.uid().to_string(),
                        to: target.clone(),
                        kind: *kind,
                        source_kind: node.kind(),
                        target_kind: target_node.kind(),
                    });
                }
            }
        }
        review_invariants::validate_review_nodes(self)?;
        supersession::validate_review_supersession(self)?;
        Ok(())
    }
}

fn canonicalize_edges(node: &mut Node) -> Result<(), CorpusError> {
    let from = node.uid().to_string();
    let edges = node.edges_mut();
    edges.sort();
    if let Some(pair) = edges.windows(2).find(|pair| pair[0] == pair[1]) {
        return Err(CorpusError::DuplicateEdge {
            from,
            to: pair[0].1.clone(),
            kind: pair[0].0,
        });
    }
    Ok(())
}

/// Sort + dedup a set-like string list into canonical form — the
/// metadata-list contract for content fields loaded from either
/// record schema (LLR-113).
pub(crate) fn canonical_strings(values: &[String]) -> Vec<String> {
    let mut values = values.to_vec();
    values.sort();
    values.dedup();
    values
}

fn edge_kinds_match(source: NodeKind, edge: EdgeKind, target: NodeKind) -> bool {
    matches!(
        (source, edge, target),
        (
            NodeKind::Requirement,
            EdgeKind::DerivesFrom,
            NodeKind::Requirement
        ) | (NodeKind::Test, EdgeKind::Verifies, NodeKind::Requirement)
            | (NodeKind::Review, EdgeKind::Reviews, NodeKind::Requirement)
            | (NodeKind::Review, EdgeKind::Supersedes, NodeKind::Review)
    )
}
