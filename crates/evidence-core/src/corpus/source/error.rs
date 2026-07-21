//! Typed errors for source-revision record loading (LLR-125).
//!
//! [`SourceError`] is the single error type of the source-revision
//! pipeline: `load_sources_into` fails closed on unreadable or
//! malformed files, newer schemas, invalid uids, invalid record
//! fields, and graph identity collisions. The record-loading
//! variants mirror their [`CorpusError`] counterparts field for
//! field so a source file and a requirement file report the same
//! degenerate input identically; [`CorpusError::Source`] wraps this
//! type so `CorpusIndex::load_graph` keeps one error type.

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
    /// Source-revision records declare no edges, so record loading
    /// cannot produce this shape; the variant mirrors the closed
    /// `insert` contract field for field.
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
