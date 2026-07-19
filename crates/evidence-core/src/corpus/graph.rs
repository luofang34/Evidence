//! Node/edge types and the uid-keyed corpus graph.
//!
//! Nodes live in a `BTreeMap` keyed by uid, so iteration — and every
//! derived view built on it — is deterministic by construction
//! (HLR-080). Edges are typed and owned by their source node; a
//! dangling target is a validation error, never a silent no-op.

use std::collections::BTreeMap;

use serde::Deserialize;

use super::error::CorpusError;

/// Typed edge kinds. Grows with the corpus model (e.g. `quotes`,
/// `reviews`, `resolves` in later milestones).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

    /// The node's outgoing typed edges.
    pub fn edges(&self) -> &[(EdgeKind, String)] {
        match self {
            Node::Requirement(r) => &r.edges,
            Node::Test(t) => &t.edges,
        }
    }
}

/// The uid-keyed corpus graph.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CorpusGraph {
    nodes: BTreeMap<String, Node>,
}

impl CorpusGraph {
    /// An empty graph.
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a node; a uid collision with any existing node — of any
    /// kind — is an error naming the uid.
    pub fn insert(&mut self, node: Node) -> Result<(), CorpusError> {
        let uid = node.uid().to_string();
        if self.nodes.contains_key(&uid) {
            return Err(CorpusError::DuplicateUid { uid });
        }
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

    /// Check every edge resolves to a node present in the graph; the
    /// first dangling edge is returned with its source, target, and
    /// kind.
    pub fn validate(&self) -> Result<(), CorpusError> {
        for node in self.nodes.values() {
            for (kind, target) in node.edges() {
                if !self.nodes.contains_key(target) {
                    return Err(CorpusError::DanglingEdge {
                        from: node.uid().to_string(),
                        to: target.clone(),
                        kind: *kind,
                    });
                }
            }
        }
        Ok(())
    }
}
