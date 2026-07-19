//! Node/edge types and the uid-keyed corpus graph.
//!
//! Nodes live in a `BTreeMap` keyed by uid, and outgoing edges are
//! sorted, so graph equality and derived views are independent of
//! input order (HLR-080). Edges are typed and owned by their source
//! node; invalid endpoints are validation errors.

use std::collections::BTreeMap;

use serde::Deserialize;

use super::error::CorpusError;

/// Typed edge kinds supported by the corpus graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EdgeKind {
    /// Requirement → parent requirement it decomposes.
    DerivesFrom,
    /// Test → requirement it verifies.
    Verifies,
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

/// Coarse node kind, for kind-filtered queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    /// A requirement at any [`RequirementLayer`].
    Requirement,
    /// A test case.
    Test,
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
    /// Primary selector displayed by legacy trace reports.
    pub(crate) primary_selector: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TraceMetadata {
    Requirement(RequirementMetadata),
    Test(TestMetadata),
}

/// A requirement node.
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

/// A corpus graph node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    /// A requirement at any layer.
    Requirement(RequirementNode),
    /// A test case.
    Test(TestNode),
}

impl Node {
    /// The node's permanent uid.
    pub fn uid(&self) -> &str {
        match self {
            Node::Requirement(r) => &r.uid,
            Node::Test(t) => &t.uid,
        }
    }

    /// The node's coarse kind.
    pub fn kind(&self) -> NodeKind {
        match self {
            Node::Requirement(_) => NodeKind::Requirement,
            Node::Test(_) => NodeKind::Test,
        }
    }

    /// The node's human-readable identifier.
    pub fn id(&self) -> &str {
        match self {
            Node::Requirement(r) => &r.id,
            Node::Test(t) => &t.id,
        }
    }

    /// The node's outgoing typed edges.
    pub fn edges(&self) -> &[(EdgeKind, String)] {
        match self {
            Node::Requirement(r) => &r.edges,
            Node::Test(t) => &t.edges,
        }
    }

    fn edges_mut(&mut self) -> &mut Vec<(EdgeKind, String)> {
        match self {
            Node::Requirement(r) => &mut r.edges,
            Node::Test(t) => &mut t.edges,
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
    /// contract.
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

fn edge_kinds_match(source: NodeKind, edge: EdgeKind, target: NodeKind) -> bool {
    matches!(
        (source, edge, target),
        (
            NodeKind::Requirement,
            EdgeKind::DerivesFrom,
            NodeKind::Requirement
        ) | (NodeKind::Test, EdgeKind::Verifies, NodeKind::Requirement)
    )
}
