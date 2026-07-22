//! The parser-independent committed structural source graph
//! (LLR-156).
//!
//! One structural source node is a strict committed record of one
//! element of a source document's structure — a section,
//! paragraph, list item, table part, code block, note, or caption
//! — bound to the frozen source revision it was parsed from. The
//! graph carries no Markdown, HTML, or PDF parser dependency:
//! ingesters produce candidates, the identity service reconciles
//! them against the committed graph, and the committed records
//! load and validate here.
//!
//! # Identity
//!
//! A node's permanent identity is its `snode_<UUIDv4>` uid,
//! distinct from the `src_<UUIDv4>` frozen document-revision
//! identity. Initial ingestion mints UUIDv4 identities through
//! [`mint_node_uid`]; re-ingestion reconciles candidates against
//! the committed graph through the deterministic structural key
//! and reuses committed uids (`identity` module, LLR-158). A
//! content change moves the content digest and produces drift;
//! it never silently replaces identity. The same uid therefore
//! recurs across revisions of one document — uid uniqueness is
//! per source revision, and the corpus holds one [`SourceGraph`]
//! per revision. Page, DOM, byte, and line positions live only
//! inside the typed locator as diagnostics, never in identity.
//!
//! # Structural invariants
//!
//! Each revision's graph is an acyclic rooted forest: parent
//! links resolve inside the same revision, sibling ordinals are
//! unique and contiguous `0..n` per parent set under canonical
//! sibling ordering (ordinal order, then uid order for ties —
//! which validation rejects), and parent/child kind pairs follow
//! the closed legality table ([`SourceNodeKind::may_parent`]).
//! Every node binds a committed source revision, and its locator
//! variant agrees with that revision's media type. Non-blank
//! labels are unique within one kind and revision — the node's
//! human identity. Validation lives in `validate` (LLR-157).
//!
//! Module map:
//!
//! - `error` — the flat [`SourceGraphError`] taxonomy every
//!   source-graph failure reports through
//! - `locator` — the closed per-format [`SourceLocator`] enum
//!   with per-variant validation (LLR-153)
//! - `normalization` — canonical prose/code text, the content
//!   digest, and the structural fingerprint (LLR-154)
//! - `records` — the strict `SourceGraphFile` serde schema and
//!   the `load_source_graphs_into` loader (LLR-152)
//! - `validate` — forest, binding, ordinal, kind, and digest
//!   invariant enforcement (LLR-157)
//! - `identity` — uid minting and structural-key reconciliation
//!   (LLR-158)
//! - `render` — the canonical byte rendering byte-locked by
//!   golden fixtures

use std::collections::BTreeMap;

use serde::Deserialize;

use self::error::SourceGraphError;
use self::locator::SourceLocator;
use super::digest::StructuralContentDigest;

pub(super) mod error;
pub(super) mod identity;
pub(super) mod locator;
pub(super) mod normalization;
pub(super) mod records;
pub(super) mod render;
pub(super) mod validate;

/// The closed structural kind of a committed source node
/// (LLR-156). Serializes snake_case on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceNodeKind {
    /// A titled container grouping its children.
    Section,
    /// A prose paragraph.
    Paragraph,
    /// One item of a list.
    ListItem,
    /// The term side of a definition-list entry.
    DefinitionTerm,
    /// The body side of a definition-list entry.
    DefinitionBody,
    /// A table, parent of rows.
    Table,
    /// A table row, parent of cells.
    TableRow,
    /// A table cell.
    TableCell,
    /// A verbatim code block; canonical text normalizes under the
    /// code contract, never prose folding.
    CodeBlock,
    /// An advisory aside (note, warning, caution).
    Note,
    /// A figure's caption.
    FigureCaption,
}

impl SourceNodeKind {
    /// The kind's snake_case wire name.
    pub fn as_str(&self) -> &'static str {
        match self {
            SourceNodeKind::Section => "section",
            SourceNodeKind::Paragraph => "paragraph",
            SourceNodeKind::ListItem => "list_item",
            SourceNodeKind::DefinitionTerm => "definition_term",
            SourceNodeKind::DefinitionBody => "definition_body",
            SourceNodeKind::Table => "table",
            SourceNodeKind::TableRow => "table_row",
            SourceNodeKind::TableCell => "table_cell",
            SourceNodeKind::CodeBlock => "code_block",
            SourceNodeKind::Note => "note",
            SourceNodeKind::FigureCaption => "figure_caption",
        }
    }

    /// The closed parent/child legality table (LLR-156): a
    /// Section parents any kind, a Table parents only TableRow, a
    /// TableRow parents only TableCell, and every other kind is a
    /// leaf. Widening the table is a contract change, never a
    /// silent edit.
    pub fn may_parent(self, child: SourceNodeKind) -> bool {
        match self {
            SourceNodeKind::Section => true,
            SourceNodeKind::Table => child == SourceNodeKind::TableRow,
            SourceNodeKind::TableRow => child == SourceNodeKind::TableCell,
            _ => false,
        }
    }
}

/// One committed structural source node (LLR-156).
///
/// The digest fields bind the canonical text and the structural
/// fingerprint exactly as stored; validation recomputes both from
/// the kind, canonical text, label, and ancestry and rejects any
/// drift. Public construction can assemble an invalid node — the
/// record loader and corpus validation are the enforcing
/// boundaries.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceNode {
    /// Permanent identity: `snode_<UUIDv4>`.
    pub uid: String,
    /// The `src_<UUIDv4>` revision this node was parsed from.
    pub source_revision_uid: String,
    /// The `snode_<UUIDv4>` parent inside the same revision;
    /// `None` for a root.
    pub parent_uid: Option<String>,
    /// Closed structural kind.
    pub kind: SourceNodeKind,
    /// Position within the parent's sibling set; unique and
    /// contiguous `0..n` per set.
    pub ordinal: u32,
    /// Optional human identity (heading text, term text);
    /// non-blank when present.
    pub label: Option<String>,
    /// Canonical text under the kind's normalization contract.
    pub canonical_text: String,
    /// SHA-256 over the canonical text encoding.
    pub content_sha256: StructuralContentDigest,
    /// SHA-256 over the stable kind/label/ancestry encoding.
    pub fingerprint: StructuralContentDigest,
    /// The one typed diagnostic locator.
    pub locator: SourceLocator,
}

/// One source revision's committed structural forest (LLR-156).
///
/// Nodes live in a `BTreeMap` keyed by uid, so iteration,
/// equality, and canonical rendering are independent of insertion
/// order, input file layout, parser event order, and map
/// iteration. Insertion enforces identity uniqueness — uid, and
/// non-blank label within one kind — while structural invariants
/// (parents, cycles, ordinals, digests) are the validator's
/// concern, mirroring the corpus graph's insert/validate split.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SourceGraph {
    nodes: BTreeMap<String, SourceNode>,
}

impl SourceGraph {
    /// An empty graph.
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a node after enforcing per-revision identity
    /// uniqueness: no duplicate uid, and no duplicate non-blank
    /// label within one kind.
    ///
    /// # Errors
    ///
    /// Fails closed with [`SourceGraphError::DuplicateUid`] or
    /// [`SourceGraphError::DuplicateHumanId`] naming the revision
    /// and both colliding uids.
    pub fn insert(&mut self, node: SourceNode) -> Result<(), SourceGraphError> {
        if let Some(existing) = self.nodes.get(&node.uid) {
            return Err(SourceGraphError::DuplicateUid {
                revision_uid: existing.source_revision_uid.clone(),
                uid: node.uid.clone(),
            });
        }
        if let Some(label) = &node.label {
            if let Some(existing) = self.nodes.values().find(|existing| {
                existing.kind == node.kind && existing.label.as_ref() == Some(label)
            }) {
                return Err(SourceGraphError::DuplicateHumanId {
                    revision_uid: node.source_revision_uid.clone(),
                    kind: node.kind,
                    label: label.clone(),
                    first_uid: existing.uid.clone(),
                    duplicate_uid: node.uid.clone(),
                });
            }
        }
        self.nodes.insert(node.uid.clone(), node);
        Ok(())
    }

    /// Look up a node by uid.
    pub fn get(&self, uid: &str) -> Option<&SourceNode> {
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
    pub fn nodes(&self) -> impl Iterator<Item = &SourceNode> {
        self.nodes.values()
    }

    /// The revision every node binds, or `None` when the graph is
    /// empty. A loaded graph is homogeneous by construction —
    /// [`CorpusGraph::insert_source_node`] routes each node to its
    /// own revision's graph.
    ///
    /// [`CorpusGraph::insert_source_node`]: super::graph::CorpusGraph
    pub fn revision_uid(&self) -> Option<&str> {
        self.nodes
            .values()
            .next()
            .map(|node| &*node.source_revision_uid)
    }
}

// Tests live in sibling files pulled in via `#[path]`: shared
// fixtures plus one module per TEST entry domain.
#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
#[path = "source_graph/tests.rs"]
mod tests;
#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
#[path = "source_graph/tests_graph/tests.rs"]
mod tests_graph;
#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
#[path = "source_graph/tests_identity/tests.rs"]
mod tests_identity;
#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
#[path = "source_graph/tests_locator/tests.rs"]
mod tests_locator;
#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
#[path = "source_graph/tests_normalization/tests.rs"]
mod tests_normalization;
#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
#[path = "source_graph/tests_support.rs"]
mod tests_support;
#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
#[path = "source_graph/tests_validation/tests.rs"]
mod tests_validation;
