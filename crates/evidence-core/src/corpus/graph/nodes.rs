//! Node and edge type declarations for the corpus graph.
//!
//! Edges are typed and owned by their source node. Node identity is
//! uid (plus human id within a kind); the review-content fields on
//! [`RequirementNode`] play no role in identity, and graph equality
//! treats layout and edge order as non-semantic (HLR-080).

use serde::{Deserialize, Serialize};

use super::super::digest::{ReviewContentDigest, SourceContentDigest};

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
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
    /// One frozen revision of a source document (LLR-126).
    SourceRevision,
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

/// The typed material state of one frozen source revision
/// (LLR-125).
///
/// The serde shape is internally tagged on `state` with unknown
/// fields denied, so the record schema and the graph node share one
/// exact wire form: an `available` state carries `retrieved_at`,
/// `sha256`, and a [`SourceCapture`]; an `unavailable` state
/// carries `reason` and nothing else — a digest on an unavailable
/// record fails deserialization rather than being invented or
/// silently dropped.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum SourceMaterial {
    /// The revision's bytes were captured; the record carries the
    /// capture proof fields.
    Available {
        /// RFC 3339 retrieval timestamp. Audit metadata only —
        /// never ordering authority between revisions.
        retrieved_at: String,
        /// Validated lowercase SHA-256 the record declares for the
        /// captured bytes.
        sha256: SourceContentDigest,
        /// How the captured bytes are held.
        capture: SourceCapture,
    },
    /// The revision's bytes could not be captured; the record
    /// carries a reason and no digest.
    Unavailable {
        /// Why the material is unavailable; non-blank.
        reason: String,
    },
}

/// The capture mode of available source material (LLR-125).
///
/// Serde snake_case with the tag named `mode` and unknown fields
/// denied; `HashOnly` is an empty struct variant so a stray field
/// fails deserialization instead of being ignored.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum SourceCapture {
    /// Raw bytes are vendored beneath the fixed `sources/` payload
    /// root; `path` is the canonical `/`-separated relative wire
    /// form, validated lexically at record load. Filesystem
    /// containment is the acquisition layer's concern, not this
    /// type's.
    Vendored {
        /// Canonical relative wire path beneath `sources/`.
        path: String,
    },
    /// Only the digest and location are recorded — for material
    /// whose redistribution is restricted.
    HashOnly {},
    /// Bytes live in an external controlled document system under
    /// an immutable identifier.
    ExternalControlled {
        /// The controlling system; non-blank.
        system: String,
        /// The immutable identifier within that system; non-blank.
        immutable_id: String,
    },
}

/// One frozen revision of a source document (LLR-126).
///
/// The node is the corpus graph object for an immutable revision:
/// `document_key` groups revisions of one logical document, and the
/// typed [`SourceMaterial`] records what was captured. Structural
/// source graph nodes are a different node kind and are not this
/// node. Source revisions carry no edges at this layer; the field
/// exists so the shared insert path can canonicalize it, and it
/// stays empty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRevisionNode {
    /// Permanent identity, unique across all node kinds.
    pub uid: String,
    /// Human-readable revision identifier, unique within the
    /// source-revision kind.
    pub id: String,
    /// Stable lineage key grouping revisions of one logical
    /// document; non-blank.
    pub document_key: String,
    /// One-line title.
    pub title: String,
    /// RFC 6838 `type/subtype` media type of the revision.
    pub media_type: String,
    /// Canonical location of the revision. Opaque audit identity:
    /// preserved exactly, never fetched or normalized as a URL.
    pub canonical_location: String,
    /// Typed material state of the revision.
    pub material: SourceMaterial,
    /// Outgoing typed edges `(kind, target uid)`. Always empty at
    /// this layer; revision-chain edges are follow-up work.
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
    /// One frozen revision of a source document.
    SourceRevision(SourceRevisionNode),
}

impl Node {
    /// The node's permanent uid.
    pub fn uid(&self) -> &str {
        match self {
            Node::Requirement(r) => &r.uid,
            Node::Test(t) => &t.uid,
            Node::Review(r) => &r.uid,
            Node::SourceRevision(s) => &s.uid,
        }
    }

    /// The node's coarse kind.
    pub fn kind(&self) -> NodeKind {
        match self {
            Node::Requirement(_) => NodeKind::Requirement,
            Node::Test(_) => NodeKind::Test,
            Node::Review(_) => NodeKind::Review,
            Node::SourceRevision(_) => NodeKind::SourceRevision,
        }
    }

    /// The node's human-readable identifier.
    pub fn id(&self) -> &str {
        match self {
            Node::Requirement(r) => &r.id,
            Node::Test(t) => &t.id,
            Node::Review(r) => &r.id,
            Node::SourceRevision(s) => &s.id,
        }
    }

    /// The node's outgoing typed edges.
    pub fn edges(&self) -> &[(EdgeKind, String)] {
        match self {
            Node::Requirement(r) => &r.edges,
            Node::Test(t) => &t.edges,
            Node::Review(r) => &r.edges,
            Node::SourceRevision(s) => &s.edges,
        }
    }

    pub(super) fn edges_mut(&mut self) -> &mut Vec<(EdgeKind, String)> {
        match self {
            Node::Requirement(r) => &mut r.edges,
            Node::Test(t) => &mut t.edges,
            Node::Review(r) => &mut r.edges,
            Node::SourceRevision(s) => &mut s.edges,
        }
    }
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
