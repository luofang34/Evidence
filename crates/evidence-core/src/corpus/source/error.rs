//! Typed errors for source-revision record loading, lineage
//! validation, and baseline transitions (LLR-125, LLR-130,
//! LLR-132).
//!
//! [`SourceError`] is the single error type of the source-revision
//! pipeline: `load_sources_into` fails closed on unreadable or
//! malformed files, newer schemas, invalid uids, invalid record
//! fields, and graph identity collisions; lineage validation fails
//! closed on self-links, cross-document links, duplicate outgoing
//! edges, forks, cycles, and multiple roots or heads for one
//! document key; and transition comparison fails closed on
//! removal, mutation, competing heads, and invalid input graphs.
//! The record-loading variants mirror their [`CorpusError`]
//! counterparts field for field so a source file and a requirement
//! file report the same degenerate input identically;
//! [`CorpusError::Source`] wraps this type so
//! `CorpusIndex::load_graph` and [`CorpusGraph::validate`] keep
//! one error type.
//!
//! [`CorpusGraph::validate`]: super::super::graph::CorpusGraph::validate

use std::path::PathBuf;

use thiserror::Error;

use super::super::error::CorpusError;
use super::super::graph::{EdgeKind, NodeKind};

/// The vendored wire-path rule a lexical check rejected
/// (LLR-125). Pure data: the failing record's [`SourceError`]
/// carries it beside the file path and the record identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum VendoredPathRule {
    /// The path is empty.
    #[error("path is empty")]
    Empty,
    /// The path is absolute (a leading `/`).
    #[error("path is absolute")]
    Absolute,
    /// The path carries a Windows drive prefix (e.g. `C:`).
    #[error("path carries a drive prefix")]
    DrivePrefix,
    /// The path carries a UNC prefix (`\\server\share`).
    #[error("path carries a UNC prefix")]
    UncPrefix,
    /// The path contains a backslash; the wire form is
    /// `/`-separated.
    #[error("path contains a backslash")]
    Backslash,
    /// The path contains an empty component (`a//b`).
    #[error("path contains an empty component")]
    EmptyComponent,
    /// The path contains a `.` component.
    #[error("path contains a `.` component")]
    DotComponent,
    /// The path contains a `..` component.
    #[error("path contains a `..` component")]
    ParentComponent,
}

/// Errors from loading source-revision records into the corpus
/// graph.
///
/// Every degenerate input fails closed with the context needed to
/// fix it; nothing is silently skipped (HLR-100, HLR-101).
#[derive(Debug, Error)]
pub enum SourceError {
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
    /// unknown field, an unknown state or mode tag, an incomplete
    /// material or capture combination, or a malformed digest —
    /// record schemas are strict).
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
    /// Two nodes claimed the same uid; uids are unique across all
    /// node kinds.
    #[error("duplicate corpus uid {uid}")]
    DuplicateUid {
        /// The colliding uid.
        uid: String,
    },
    /// Two nodes of the same kind claimed the same human identifier.
    #[error("duplicate {kind:?} id {id:?}: first uid {first_uid}, duplicate uid {duplicate_uid}")]
    DuplicateHumanId {
        /// The colliding human identifier.
        id: String,
        /// Node kind within which the identifier must be unique.
        kind: NodeKind,
        /// Uid of the node inserted first.
        first_uid: String,
        /// Uid of the rejected node.
        duplicate_uid: String,
    },
    /// One node declared the same typed edge more than once.
    /// Source-revision records name a single optional `supersedes`
    /// target, so record loading cannot produce this shape; the
    /// variant mirrors the closed `insert` contract field for
    /// field.
    ///
    /// [`insert`]: super::super::graph::CorpusGraph::insert
    #[error("duplicate {kind:?} edge from {from} to {to}")]
    DuplicateEdge {
        /// Uid of the node that owns the edge.
        from: String,
        /// Duplicate target uid.
        to: String,
        /// Edge kind.
        kind: EdgeKind,
    },
    /// A source record's human identifier is empty; every record
    /// needs one for audit cross-reference (LLR-125).
    #[error("source record with uid {uid} in {path} has an empty human id")]
    SourceHumanId {
        /// Source file path.
        path: PathBuf,
        /// The record's uid.
        uid: String,
    },
    /// A source record's document lineage key is blank; revisions of
    /// one logical document group under a stable non-empty key
    /// (LLR-125).
    #[error("source record {id:?} ({uid}) in {path} has a blank document key")]
    SourceDocumentKey {
        /// Source file path.
        path: PathBuf,
        /// The record's uid.
        uid: String,
        /// The record's human identifier.
        id: String,
    },
    /// A source record's title is blank (LLR-125).
    #[error("source record {id:?} ({uid}) in {path} has a blank title")]
    SourceTitle {
        /// Source file path.
        path: PathBuf,
        /// The record's uid.
        uid: String,
        /// The record's human identifier.
        id: String,
    },
    /// A source record's media type is not in RFC 6838
    /// `type/subtype` token form (LLR-125).
    #[error(
        "source record {id:?} ({uid}) in {path} has media type {value:?}, \
         which is not in RFC 6838 type/subtype form"
    )]
    SourceMediaType {
        /// Source file path.
        path: PathBuf,
        /// The record's uid.
        uid: String,
        /// The record's human identifier.
        id: String,
        /// The offending media type string.
        value: String,
    },
    /// A source record's canonical location is blank; the location
    /// is opaque audit identity and must be present (LLR-125).
    #[error("source record {id:?} ({uid}) in {path} has a blank canonical location")]
    SourceCanonicalLocation {
        /// Source file path.
        path: PathBuf,
        /// The record's uid.
        uid: String,
        /// The record's human identifier.
        id: String,
    },
    /// An available source record's `retrieved_at` does not parse
    /// as an RFC 3339 timestamp (LLR-125).
    #[error(
        "source record {id:?} ({uid}) in {path} has retrieved_at {value:?}, \
         which is not an RFC 3339 timestamp"
    )]
    SourceTimestamp {
        /// Source file path.
        path: PathBuf,
        /// The record's uid.
        uid: String,
        /// The record's human identifier.
        id: String,
        /// The offending timestamp string.
        value: String,
    },
    /// An unavailable source record carries a blank reason
    /// (LLR-125).
    #[error("unavailable source record {id:?} ({uid}) in {path} has a blank reason")]
    SourceReason {
        /// Source file path.
        path: PathBuf,
        /// The record's uid.
        uid: String,
        /// The record's human identifier.
        id: String,
    },
    /// An external-controlled capture names a blank controlling
    /// system (LLR-125).
    #[error(
        "external-controlled source record {id:?} ({uid}) in {path} names a blank controlling system"
    )]
    SourceCaptureSystem {
        /// Source file path.
        path: PathBuf,
        /// The record's uid.
        uid: String,
        /// The record's human identifier.
        id: String,
    },
    /// An external-controlled capture names a blank immutable
    /// identifier (LLR-125).
    #[error(
        "external-controlled source record {id:?} ({uid}) in {path} names a blank immutable identifier"
    )]
    SourceCaptureImmutableId {
        /// Source file path.
        path: PathBuf,
        /// The record's uid.
        uid: String,
        /// The record's human identifier.
        id: String,
    },
    /// A vendored capture path is not in canonical `/`-separated
    /// relative wire form beneath the `sources/` payload root
    /// (LLR-125).
    #[error("source record {id:?} ({uid}) in {path} has vendored path {value:?}: {rule}")]
    SourceVendoredPath {
        /// Source file path.
        path: PathBuf,
        /// The record's uid.
        uid: String,
        /// The record's human identifier.
        id: String,
        /// The offending wire path.
        value: String,
        /// The lexical rule the path violated.
        rule: VendoredPathRule,
    },
    /// A source revision named itself as its own supersedes target;
    /// a revision supersedes a strictly prior revision (LLR-130).
    #[error("source revision {uid} supersedes itself")]
    SourceSupersessionSelf {
        /// The offending revision's uid.
        uid: String,
    },
    /// A source revision superseded a revision of a different
    /// document key; supersession links only revisions of one
    /// logical document (LLR-130).
    #[error(
        "source revision {uid} supersedes {predecessor_uid}, which belongs to a different document key"
    )]
    SourceSupersessionDocumentKey {
        /// The superseding revision's uid.
        uid: String,
        /// The prior revision's uid.
        predecessor_uid: String,
    },
    /// A source revision owned more than one outgoing supersedes
    /// edge; a revision supersedes at most one predecessor
    /// (LLR-130). The record loader cannot produce this shape (a
    /// record names a single optional `supersedes`), so this
    /// invariant guards programmatically built graphs.
    #[error(
        "source revision {source_uid} owns {count} supersedes edges; \
         a revision supersedes at most one predecessor"
    )]
    SourceDuplicateSupersedesEdge {
        /// Uid of the revision that owns the edges.
        source_uid: String,
        /// Number of outgoing supersedes edges found.
        count: usize,
    },
    /// A source revision is superseded by more than one other
    /// revision — a fork in the document lineage (LLR-130). This is
    /// the dual direction of the per-revision outgoing check: a
    /// revision supersedes at most one predecessor, and a
    /// predecessor is superseded by at most one revision.
    #[error(
        "source revision {uid} is superseded by both {first_uid} and {second_uid}; \
         a revision is superseded by at most one successor"
    )]
    SourceSupersessionFork {
        /// The forked revision's uid.
        uid: String,
        /// Uid of the first superseding revision (uid order).
        first_uid: String,
        /// Uid of the second superseding revision (uid order).
        second_uid: String,
    },
    /// Walking a source supersession chain revisited a revision;
    /// every document lineage is acyclic (LLR-130).
    #[error("source supersession cycle detected at revision {uid}")]
    SourceSupersessionCycle {
        /// A revision uid on the cycle.
        uid: String,
    },
    /// One document key has more than one root revision — a
    /// revision that supersedes nothing — so the lineage is not a
    /// single chain (LLR-130).
    #[error(
        "document key {document_key} has multiple unrelated roots {first_uid} and {second_uid}; \
         a document lineage is a single chain with exactly one root"
    )]
    SourceLineageMultipleRoots {
        /// The document key with multiple roots.
        document_key: String,
        /// Uid of the first root revision (uid order).
        first_uid: String,
        /// Uid of the second root revision (uid order).
        second_uid: String,
    },
    /// One document key has more than one effective head — a
    /// revision no other revision supersedes — so the lineage has
    /// no single newest revision (LLR-130). Defense in depth: an
    /// acyclic lineage with at most one edge per direction has
    /// equally many roots and heads, so
    /// [`SourceError::SourceLineageMultipleRoots`] reports the
    /// violating lineage first through the public validators.
    #[error(
        "document key {document_key} has multiple effective heads {first_uid} and {second_uid}; \
         a document lineage has exactly one effective head"
    )]
    SourceLineageMultipleHeads {
        /// The document key with multiple heads.
        document_key: String,
        /// Uid of the first head revision (uid order).
        first_uid: String,
        /// Uid of the second head revision (uid order).
        second_uid: String,
    },
    /// A graph failed [`CorpusGraph::validate`] before a source
    /// transition comparison could run on it
    /// (LLR-132). [`validate_source_transition`] validates the
    /// prior graph before comparing and the proposed graph after
    /// comparing, and fails closed with the [`CorpusError`]
    /// carried as the typed source (never stringified), so callers
    /// can match the exact broken invariant. Boxed because
    /// `CorpusError` is large enough to trip `result_large_err` on
    /// some platforms.
    ///
    /// [`CorpusGraph::validate`]: super::super::graph::CorpusGraph::validate
    /// [`validate_source_transition`]: super::lineage::validate_source_transition
    #[error("{graph} corpus graph failed validation: {source}")]
    SourceTransitionInvalidGraph {
        /// Which graph failed: `"prior"` or `"proposed"`.
        graph: &'static str,
        /// The validation error, carried whole.
        #[source]
        source: Box<CorpusError>,
    },
    /// A source revision of the prior baseline is absent from the
    /// proposed graph; revisions are immutable and can never be
    /// silently removed (LLR-132).
    #[error(
        "source revision {uid} of the prior baseline is absent from the proposed graph; \
         revisions are immutable and cannot be removed"
    )]
    SourceTransitionRemoval {
        /// The removed revision's uid.
        uid: String,
    },
    /// A source revision's immutable projection differs between
    /// the prior and proposed graphs; revisions are immutable and
    /// can never be edited in place, so new bytes require a new
    /// `src_` uid and a supersedes edge (LLR-132).
    #[error(
        "source revision {uid} differs in field {field}; \
         revisions are immutable and cannot be edited in place"
    )]
    SourceTransitionMutation {
        /// The mutated revision's uid.
        uid: String,
        /// The projection field that differs.
        field: &'static str,
    },
    /// A new source revision joining an existing document key does
    /// not supersede that document's prior effective head; a new
    /// revision must extend the prior head, never compete with it
    /// (LLR-132).
    #[error(
        "source revision {uid} joins document key {document_key} without superseding \
         the prior effective head {prior_head_uid}; a new revision must extend the prior head"
    )]
    SourceTransitionCompetingHead {
        /// The competing revision's uid.
        uid: String,
        /// The document key it joins.
        document_key: String,
        /// Uid of the prior effective head it had to supersede.
        prior_head_uid: String,
    },
    /// `CorpusGraph::insert` failed with a variant outside its
    /// closed error contract. The contract — identity collisions
    /// and duplicate edges only — makes this unreachable today,
    /// but a contract change upstream must degrade to a typed
    /// error carrying the original [`CorpusError`] as its source,
    /// never a panic (LLR-125). Boxed because `CorpusError` is
    /// large enough to trip `result_large_err` on some platforms;
    /// the source object is carried whole either way.
    ///
    /// [`CorpusGraph::insert`]: super::super::graph::CorpusGraph::insert
    #[error("unexpected graph insertion error: {0}")]
    UnexpectedInsertError(#[source] Box<CorpusError>),
}

impl SourceError {
    /// Lift a [`CorpusGraph::insert`] failure into the source error
    /// type. `insert` fails only on identity collisions and duplicate
    /// edges, which map field for field; any other variant is outside
    /// that closed contract today and is preserved whole in
    /// [`SourceError::UnexpectedInsertError`] rather than panicking.
    ///
    /// [`CorpusGraph::insert`]: super::super::graph::CorpusGraph::insert
    pub(super) fn from_insert(err: CorpusError) -> Self {
        match err {
            CorpusError::DuplicateUid { uid } => SourceError::DuplicateUid { uid },
            CorpusError::DuplicateHumanId {
                id,
                kind,
                first_uid,
                duplicate_uid,
            } => SourceError::DuplicateHumanId {
                id,
                kind,
                first_uid,
                duplicate_uid,
            },
            CorpusError::DuplicateEdge { from, to, kind } => {
                SourceError::DuplicateEdge { from, to, kind }
            }
            other => SourceError::UnexpectedInsertError(Box::new(other)),
        }
    }
}
