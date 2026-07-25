//! The uid-keyed corpus graph.
//!
//! Nodes live in a `BTreeMap` keyed by uid, and outgoing edges are
//! sorted, so graph equality and derived views are independent of
//! input order (HLR-080). Edges are typed and owned by their source
//! node; invalid endpoints are validation errors.
//!
//! Module map:
//!
//! - `nodes` — node/edge type and trace-metadata declarations
//! - `validation` — identity uniqueness, edge canonicalization, and
//!   edge endpoint validation
//! - `review_invariants` — per-node review invariants (LLR-115)
//! - `supersession` — review supersession chain validation (LLR-115)

use std::collections::{BTreeMap, BTreeSet};

use super::error::CorpusError;
use super::source_graph::error::SourceGraphError;
use super::source_graph::{SourceGraph, SourceNode};
use super::source_patch::error::SourcePatchError;
use super::source_patch::records::SourcePatchRecord;

mod nodes;
mod review_invariants;
mod supersession;
mod validation;

pub use nodes::{
    EdgeKind, Node, NodeKind, RequirementLayer, RequirementNode, ReviewDecision, ReviewNode,
    ReviewTarget, ReviewTargetKind, SourceCapture, SourceMaterial, SourceRevisionNode, TestNode,
};
pub(crate) use nodes::{RequirementMetadata, TestMetadata, TraceMetadata, canonical_strings};

/// The uid-keyed corpus graph.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CorpusGraph {
    nodes: BTreeMap<String, Node>,
    trace_metadata: BTreeMap<String, TraceMetadata>,
    source_graphs: BTreeMap<String, SourceGraph>,
    source_patches: BTreeMap<String, SourcePatchRecord>,
}

impl CorpusGraph {
    /// An empty graph.
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a node after enforcing identity uniqueness and
    /// canonical, duplicate-free edge order.
    pub fn insert(&mut self, mut node: Node) -> Result<(), CorpusError> {
        validation::check_identity_uniqueness(&self.nodes, &node)?;
        validation::canonicalize_edges(&mut node)?;
        self.nodes.insert(node.uid().to_string(), node);
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

    /// The committed structural source graphs, keyed by
    /// source-revision uid (LLR-156). Read-only view; loading goes
    /// through the corpus index.
    pub fn source_graphs(&self) -> &BTreeMap<String, SourceGraph> {
        &self.source_graphs
    }

    /// The committed structural source graph of one source
    /// revision, when any of its nodes have loaded.
    pub fn source_graph(&self, source_revision_uid: &str) -> Option<&SourceGraph> {
        self.source_graphs.get(source_revision_uid)
    }

    /// Route one structural source node into its own revision's
    /// committed graph, creating the graph on first use. Identity
    /// uniqueness is per revision — the same `snode_` uid recurs
    /// across revisions of one document by design.
    pub(crate) fn insert_source_node(&mut self, node: SourceNode) -> Result<(), SourceGraphError> {
        let revision_uid = node.source_revision_uid.clone();
        self.source_graphs
            .entry(revision_uid)
            .or_default()
            .insert(node)
    }

    /// The committed curated patches, keyed by `patch_` uid
    /// (LLR-169). Read-only view: the patch plane sits beside the
    /// parser-output source graphs, separately inspectable, so
    /// curated content can never be mistaken for raw extraction.
    pub fn source_patches(&self) -> &BTreeMap<String, SourcePatchRecord> {
        &self.source_patches
    }

    /// The committed curated patch with `uid`, when loaded.
    pub fn source_patch(&self, uid: &str) -> Option<&SourcePatchRecord> {
        self.source_patches.get(uid)
    }

    /// Insert one curated patch after enforcing identity
    /// uniqueness within the curated-patch kind: no duplicate uid,
    /// no duplicate human id.
    pub(crate) fn insert_source_patch(
        &mut self,
        patch: SourcePatchRecord,
    ) -> Result<(), SourcePatchError> {
        if self.source_patches.contains_key(&patch.uid) {
            return Err(SourcePatchError::DuplicateUid {
                uid: patch.uid.clone(),
            });
        }
        if let Some(existing) = self
            .source_patches
            .values()
            .find(|existing| existing.human_id == patch.human_id)
        {
            return Err(SourcePatchError::DuplicateHumanId {
                human_id: patch.human_id.clone(),
                first_uid: existing.uid.clone(),
                duplicate_uid: patch.uid.clone(),
            });
        }
        self.source_patches.insert(patch.uid.clone(), patch);
        Ok(())
    }

    /// All review nodes targeting the requirement
    /// `requirement_uid`, in uid order (LLR-118). Read-only derived
    /// view: supersession and decision semantics are the lifecycle
    /// evaluator's concern, not this accessor's.
    pub fn reviews_for_requirement(&self, requirement_uid: &str) -> Vec<&ReviewNode> {
        self.nodes
            .values()
            .filter_map(|node| match node {
                Node::Review(review)
                    if matches!(&review.target, ReviewTarget::Requirement(uid) if uid == requirement_uid) =>
                {
                    Some(review)
                }
                _ => None,
            })
            .collect()
    }

    /// All review nodes targeting the curated patch `patch_uid`,
    /// in uid order (LLR-173). Read-only derived view, mirroring
    /// [`CorpusGraph::reviews_for_requirement`].
    pub fn reviews_for_patch(&self, patch_uid: &str) -> Vec<&ReviewNode> {
        self.nodes
            .values()
            .filter_map(|node| match node {
                Node::Review(review)
                    if matches!(&review.target, ReviewTarget::CuratedPatch(uid) if uid == patch_uid) =>
                {
                    Some(review)
                }
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
    /// chains (LLR-115), then validate source-revision lineage
    /// chains (LLR-130), then validate the committed structural
    /// source graphs (LLR-157), then validate the committed
    /// curated patches against those graphs (LLR-169). Review failures surface as
    /// [`CorpusError::Review`] wrapping the typed review error;
    /// source lineage failures surface as [`CorpusError::Source`]
    /// wrapping the typed source error; source-graph failures
    /// surface as [`CorpusError::SourceGraph`] wrapping the typed
    /// source-graph error.
    pub fn validate(&self) -> Result<(), CorpusError> {
        validation::validate_edges(self)?;
        review_invariants::validate_review_nodes(self).map_err(Box::new)?;
        supersession::validate_review_supersession(self).map_err(Box::new)?;
        super::source::lineage::validate_source_lineage(self)?;
        super::source_graph::validate::validate_source_graphs(self)?;
        super::source_patch::validate::validate_source_patches(self).map_err(Box::new)?;
        Ok(())
    }
}
