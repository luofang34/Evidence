//! Typed errors for source-graph record loading, structural
//! invariant validation, and locator validation (LLR-152,
//! LLR-153, LLR-157).
//!
//! [`SourceGraphError`] is the single error type of the
//! structural source-graph pipeline: `load_source_graphs_into`
//! fails closed on unreadable or malformed files, newer schemas,
//! invalid uids, blank labels, and invalid locator fields;
//! insertion fails closed on duplicate uids and duplicate human
//! identities within one revision; and graph validation fails
//! closed on dangling or cross-revision parents, cycles, illegal
//! parent/child kind combinations, duplicate or gapped sibling
//! ordinals, digest and fingerprint mismatches, unbound source
//! revisions, and locator/media disagreement. The record-loading
//! variants mirror their [`CorpusError`] counterparts field for
//! field so a source-graph file and a requirement file report the
//! same degenerate input identically;
//! [`CorpusError::SourceGraph`] wraps this type so
//! `CorpusIndex::load_graph` and [`CorpusGraph::validate`] keep
//! one error type.
//!
//! [`CorpusError`]: super::super::error::CorpusError
//! [`CorpusError::SourceGraph`]: super::super::error::CorpusError::SourceGraph
//! [`CorpusIndex::load_graph`]: super::super::index::CorpusIndex::load_graph
//! [`CorpusGraph::validate`]: super::super::graph::CorpusGraph::validate

use std::path::PathBuf;

use thiserror::Error;

use super::SourceNodeKind;
use super::locator::LocatorRule;

/// Errors from loading source-graph records into the corpus graph
/// and from validating the committed structural forest.
///
/// Every degenerate input fails closed with the context needed to
/// fix it — the source revision, the node uid, the field, and the
/// conflicting values; nothing is silently skipped (HLR-117,
/// HLR-119).
#[derive(Debug, Error)]
pub enum SourceGraphError {
    /// Failed to read a corpus record file named by the index.
    #[error("reading corpus record file {path}")]
    RecordRead {
        /// Record file path.
        path: PathBuf,
        /// Underlying OS error.
        #[source]
        source: std::io::Error,
    },
    /// A corpus record file did not parse (malformed TOML, an
    /// unknown field, an unknown kind or format tag, a malformed
    /// digest, or an unsafe locator path — record schemas are
    /// strict).
    #[error("parsing corpus record file {path}")]
    RecordParse {
        /// Record file path.
        path: PathBuf,
        /// Underlying TOML error.
        #[source]
        source: toml::de::Error,
    },
    /// A record file declares a schema newer than this tool supports.
    #[error(
        "corpus record file {path} declares schema_version {found}; \
         this tool supports up to {supported}"
    )]
    RecordSchemaTooNew {
        /// Record file path.
        path: PathBuf,
        /// Declared `schema_version`.
        found: u32,
        /// Highest version this tool loads.
        supported: u32,
    },
    /// A corpus-native record's uid lacks its kind's typed prefix.
    #[error("corpus-native uid {uid:?} must start with {expected:?}")]
    NativeUidPrefix {
        /// The offending uid.
        uid: String,
        /// Required prefix for the record's kind.
        expected: &'static str,
    },
    /// A corpus-native uid suffix is not an RFC 9562 UUIDv4.
    #[error("corpus-native uid {uid:?} must end with an RFC 9562 UUIDv4")]
    NativeUidUuidV4 {
        /// The offending uid.
        uid: String,
    },
    /// Two nodes of one source revision claimed the same uid.
    #[error("duplicate source-node uid {uid} in source revision {revision_uid}")]
    DuplicateUid {
        /// Revision the colliding nodes belong to.
        revision_uid: String,
        /// The colliding uid.
        uid: String,
    },
    /// Two nodes of one kind in one source revision claimed the
    /// same label — the structural node's human identity.
    #[error(
        "duplicate source-node label {label:?} for kind {kind:?} in source revision \
         {revision_uid}: first uid {first_uid}, duplicate uid {duplicate_uid}"
    )]
    DuplicateHumanId {
        /// Revision the colliding nodes belong to.
        revision_uid: String,
        /// Node kind within which the label must be unique.
        kind: SourceNodeKind,
        /// The colliding label.
        label: String,
        /// Uid of the node inserted first.
        first_uid: String,
        /// Uid of the rejected node.
        duplicate_uid: String,
    },
    /// A node record's label is blank; a present label is the
    /// node's human identity and must carry content (LLR-152).
    #[error("source node {uid} in {path} has a blank label")]
    NodeLabel {
        /// Record file path.
        path: PathBuf,
        /// The record's uid.
        uid: String,
    },
    /// A locator field violates its per-variant rule (LLR-153).
    #[error("source node {node_uid} in {path} has invalid {field} value {value:?}: {rule}")]
    InvalidLocatorField {
        /// Record file path.
        path: PathBuf,
        /// The record's uid.
        node_uid: String,
        /// The offending field's wire name.
        field: &'static str,
        /// The offending value, rendered for diagnostics.
        value: String,
        /// The per-variant rule the value violated.
        rule: LocatorRule,
    },
    /// A node's parent uid is absent from every committed source
    /// graph.
    #[error(
        "source node {node_uid} in source revision {revision_uid} names parent {parent_uid}, \
         which is absent from the committed graph"
    )]
    DanglingParent {
        /// Revision the node belongs to.
        revision_uid: String,
        /// The node carrying the link.
        node_uid: String,
        /// The missing parent uid.
        parent_uid: String,
    },
    /// A node's parent uid exists only in a different source
    /// revision; parent links stay within one revision.
    #[error(
        "source node {node_uid} in source revision {revision_uid} names parent {parent_uid} \
         of source revision {parent_revision_uid}; parent links stay within one source revision"
    )]
    CrossRevisionParent {
        /// Revision the node belongs to.
        revision_uid: String,
        /// The node carrying the link.
        node_uid: String,
        /// The parent uid.
        parent_uid: String,
        /// Revision the parent actually belongs to.
        parent_revision_uid: String,
    },
    /// Walking a parent chain revisited a node; every source graph
    /// is an acyclic rooted forest.
    #[error(
        "source-node parent cycle detected at node {node_uid} in source revision {revision_uid}"
    )]
    Cycle {
        /// Revision the cycle belongs to.
        revision_uid: String,
        /// A node uid on the cycle.
        node_uid: String,
    },
    /// A parent/child kind pair violates the closed legality
    /// table.
    #[error(
        "source node {node_uid} of kind {kind:?} in source revision {revision_uid} cannot be \
         parented by {parent_uid} of kind {parent_kind:?}"
    )]
    IllegalParentKind {
        /// Revision the node belongs to.
        revision_uid: String,
        /// The child node.
        node_uid: String,
        /// The child node's kind.
        kind: SourceNodeKind,
        /// The parent node.
        parent_uid: String,
        /// The parent node's kind.
        parent_kind: SourceNodeKind,
    },
    /// Two siblings under one parent claimed the same ordinal.
    #[error(
        "duplicate sibling ordinal {ordinal} under parent {parent_uid:?} in source revision \
         {revision_uid}: first uid {first_uid}, duplicate uid {duplicate_uid}"
    )]
    DuplicateOrdinal {
        /// Revision the siblings belong to.
        revision_uid: String,
        /// The shared parent uid; `None` names the root set.
        parent_uid: Option<String>,
        /// The colliding ordinal.
        ordinal: u32,
        /// Uid of the sibling inserted first.
        first_uid: String,
        /// Uid of the rejected sibling.
        duplicate_uid: String,
    },
    /// A sibling set's ordinals are not contiguous `0..n` after
    /// canonical sibling ordering.
    #[error(
        "sibling ordinals under parent {parent_uid:?} in source revision {revision_uid} are not \
         contiguous: expected ordinal {expected}, found {found} on node {node_uid}"
    )]
    NonContiguousOrdinals {
        /// Revision the siblings belong to.
        revision_uid: String,
        /// The shared parent uid; `None` names the root set.
        parent_uid: Option<String>,
        /// The ordinal the canonical sequence expected.
        expected: u32,
        /// The ordinal the node carried instead.
        found: u32,
        /// The node whose ordinal broke the sequence.
        node_uid: String,
    },
    /// A stored digest does not match the value recomputed from
    /// the committed node's kind, canonical text, label, and
    /// ancestry (LLR-157).
    #[error(
        "source node {node_uid} in source revision {revision_uid} has {field} {actual}, \
         which does not match the recomputed value {expected}"
    )]
    DigestMismatch {
        /// Revision the node belongs to.
        revision_uid: String,
        /// The node whose digest drifted.
        node_uid: String,
        /// The digest field: `content_sha256` or `fingerprint`.
        field: &'static str,
        /// The recomputed value the graph should carry.
        expected: String,
        /// The value the record stored.
        actual: String,
    },
    /// A source graph's revision uid names no committed
    /// source-revision node; every structural node binds a frozen
    /// revision.
    #[error(
        "source graph for revision {revision_uid} names no committed source revision; \
         structural nodes bind a frozen source revision"
    )]
    UnknownSourceRevision {
        /// The unbound revision uid.
        revision_uid: String,
    },
    /// A node's locator variant disagrees with its revision's
    /// declared media type under the closed Markdown/HTML/PDF
    /// mapping.
    #[error(
        "source node {node_uid} in source revision {revision_uid} has a {locator_format} \
         locator, which disagrees with the revision's media type {media_type:?}"
    )]
    LocatorMediaMismatch {
        /// Revision the node belongs to.
        revision_uid: String,
        /// The node carrying the locator.
        node_uid: String,
        /// The locator variant's `format` wire string.
        locator_format: &'static str,
        /// The revision's declared media type.
        media_type: String,
    },
}
