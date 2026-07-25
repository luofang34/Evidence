//! Typed errors for curated-patch record loading, corpus-level
//! patch validation, and candidate patch application (LLR-166,
//! LLR-168, LLR-169).
//!
//! [`SourcePatchError`] is the single error type of the
//! curated-patch pipeline: the record loader fails closed on
//! unreadable or malformed files, newer schemas, malformed uids,
//! blank metadata, duplicate or conflicting operations, and a
//! reviewed-content digest that does not recompute; corpus
//! validation fails closed on unbound revisions, stale pre-patch
//! graph digests, cross-document and dangling targets, and
//! implicit cross-patch references; and candidate application
//! fails closed on an invalid parser graph, stale bindings, stale
//! preconditions, inserted-identity collisions, and an invalid
//! post-patch graph. [`CorpusError::SourcePatch`] wraps this type
//! so `CorpusIndex::load_graph` and [`CorpusGraph::validate`]
//! keep one error type.
//!
//! [`CorpusError::SourcePatch`]: super::super::error::CorpusError::SourcePatch
//! [`CorpusIndex::load_graph`]: super::super::index::CorpusIndex::load_graph
//! [`CorpusGraph::validate`]: super::super::graph::CorpusGraph::validate

use std::path::PathBuf;

use thiserror::Error;

use super::super::source_graph::error::SourceGraphError;
use super::super::source_graph::locator::LocatorRule;

/// Errors from loading curated-patch records, validating the
/// committed patch plane, and applying a patch candidate.
///
/// Every degenerate input fails closed with the context needed to
/// fix it — the patch uid, the operation ordinal, the target uid,
/// the field, and the conflicting values; nothing is silently
/// skipped (HLR-126, HLR-127, HLR-128).
#[derive(Debug, Error)]
pub enum SourcePatchError {
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
    /// unknown field, an unknown operation tag or kind, or a
    /// malformed digest — record schemas are strict).
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
    /// Two curated patches claimed the same `patch_` uid.
    #[error("duplicate curated-patch uid {uid}")]
    DuplicateUid {
        /// The colliding uid.
        uid: String,
    },
    /// Two curated patches claimed the same human id — the patch's
    /// human identity, unique within the curated-patch kind.
    #[error(
        "duplicate curated-patch human id {human_id:?}: first uid {first_uid}, \
         duplicate uid {duplicate_uid}"
    )]
    DuplicateHumanId {
        /// The colliding human id.
        human_id: String,
        /// Uid of the patch inserted first.
        first_uid: String,
        /// Uid of the rejected patch.
        duplicate_uid: String,
    },
    /// A record field that must carry content is blank.
    #[error("curated patch {uid} in {path} has a blank {field}")]
    BlankField {
        /// Record file path.
        path: PathBuf,
        /// The patch's uid.
        uid: String,
        /// The blank field's wire name.
        field: &'static str,
    },
    /// A record's `created_at` is not RFC 3339.
    #[error("curated patch {uid} in {path} has created_at {value:?}, which is not RFC 3339")]
    PatchTimestamp {
        /// Record file path.
        path: PathBuf,
        /// The patch's uid.
        uid: String,
        /// The offending timestamp string.
        value: String,
    },
    /// A patch declares no operations; a curated patch exists to
    /// change the graph.
    #[error("curated patch {uid} in {path} declares no operations")]
    EmptyOperations {
        /// Record file path.
        path: PathBuf,
        /// The patch's uid.
        uid: String,
    },
    /// Two operations claimed the same ordinal; application order
    /// must be unambiguous.
    #[error("curated patch {uid} in {path} has duplicate operation ordinal {ordinal}")]
    DuplicateOperationOrdinal {
        /// Record file path.
        path: PathBuf,
        /// The patch's uid.
        uid: String,
        /// The colliding ordinal.
        ordinal: u32,
    },
    /// Two operations of the same kind target the same node — a
    /// duplicate or conflicting pair.
    #[error("curated patch {uid} in {path} has conflicting {op} operations on target {target_uid}")]
    ConflictingOperation {
        /// Record file path.
        path: PathBuf,
        /// The patch's uid.
        uid: String,
        /// The operation wire tag.
        op: &'static str,
        /// The conflicting target uid.
        target_uid: String,
    },
    /// A `replace_content` operation declares neither new canonical
    /// text nor a new label.
    #[error(
        "curated patch {uid} in {path} has a replace_content operation at ordinal {ordinal} \
         with neither new_canonical_text nor new_label"
    )]
    IncompleteReplaceContent {
        /// Record file path.
        path: PathBuf,
        /// The patch's uid.
        uid: String,
        /// The operation's ordinal.
        ordinal: u32,
    },
    /// An inserted node spec's locator field violates its
    /// per-variant rule.
    #[error(
        "curated patch {uid} in {path} has an inserted node with invalid {field} value \
         {value:?}: {rule}"
    )]
    InvalidLocatorField {
        /// Record file path.
        path: PathBuf,
        /// The patch's uid.
        uid: String,
        /// The offending field's wire name.
        field: &'static str,
        /// The offending value, rendered for diagnostics.
        value: String,
        /// The per-variant rule the value violated.
        rule: LocatorRule,
    },
    /// The stored reviewed-content digest does not match the value
    /// recomputed from the bindings and the ordered operations.
    #[error(
        "curated patch {uid} in {path} has reviewed_content_digest {actual}, which does not \
         match the recomputed value {expected}"
    )]
    ReviewedContentDigestMismatch {
        /// Record file path.
        path: PathBuf,
        /// The patch's uid.
        uid: String,
        /// The recomputed value the record should carry.
        expected: String,
        /// The value the record stored.
        actual: String,
    },
    /// A patch's source-revision uid names no committed
    /// source-revision node.
    #[error(
        "curated patch {patch_uid} binds source revision {revision_uid}, which names no \
         committed source revision"
    )]
    UnknownSourceRevision {
        /// The patch's uid.
        patch_uid: String,
        /// The unbound revision uid.
        revision_uid: String,
    },
    /// A patch's pre-patch graph digest does not match the canonical
    /// digest of the bound revision's committed parser graph.
    #[error(
        "curated patch {patch_uid} has pre_patch_graph_digest {actual}, which does not match \
         the committed parser graph's canonical digest {expected}"
    )]
    PrePatchGraphDigestMismatch {
        /// The patch's uid.
        patch_uid: String,
        /// The committed graph's canonical digest.
        expected: String,
        /// The value the record stored.
        actual: String,
    },
    /// An operation target resolves only in a different source
    /// revision; patches edit exactly one revision's graph.
    #[error(
        "curated patch {patch_uid} targets node {target_uid} of source revision \
         {target_revision_uid}; cross-document edits are rejected"
    )]
    CrossRevisionTarget {
        /// The patch's uid.
        patch_uid: String,
        /// The offending target uid.
        target_uid: String,
        /// The revision the target actually belongs to.
        target_revision_uid: String,
    },
    /// An operation target resolves only as another patch's inserted
    /// node; implicit cross-patch cascades are rejected.
    #[error(
        "curated patch {patch_uid} targets node {target_uid} inserted by curated patch \
         {other_patch_uid}; implicit cross-patch cascades are rejected"
    )]
    CrossPatchTarget {
        /// The patch's uid.
        patch_uid: String,
        /// The offending target uid.
        target_uid: String,
        /// The patch that inserts the target.
        other_patch_uid: String,
    },
    /// An operation target resolves nowhere — not in the bound
    /// revision's graph and not among the patch's own inserted
    /// nodes.
    #[error("curated patch {patch_uid} targets node {target_uid}, which resolves nowhere")]
    DanglingTarget {
        /// The patch's uid.
        patch_uid: String,
        /// The dangling target uid.
        target_uid: String,
    },
    /// A binding digest presented at candidate application does not
    /// match the patch record; the patch is stale against the
    /// recipe, input, or pre-patch graph it was curated for.
    #[error(
        "curated patch {patch_uid} is stale: {field} is {actual}, but the patch was curated \
         against {expected}"
    )]
    StaleBinding {
        /// The patch's uid.
        patch_uid: String,
        /// The binding field: `recipe_digest`, `input_digest`, or
        /// `pre_patch_graph_digest`.
        field: &'static str,
        /// The digest the patch was curated against.
        expected: String,
        /// The digest presented at application.
        actual: String,
    },
    /// An operation's precondition does not match the graph state at
    /// its application point; the operation fails closed.
    #[error(
        "curated patch {patch_uid} operation at ordinal {ordinal} on target {target_uid} has \
         stale {field}: expected {expected}, found {actual}"
    )]
    StalePrecondition {
        /// The patch's uid.
        patch_uid: String,
        /// The operation's ordinal.
        ordinal: u32,
        /// The operation's target uid.
        target_uid: String,
        /// The precondition field.
        field: &'static str,
        /// The value the operation expected.
        expected: String,
        /// The value the graph carried.
        actual: String,
    },
    /// An inserted node's uid or non-blank label collides with a
    /// node already in the graph.
    #[error(
        "curated patch {patch_uid} inserts node {uid}, whose {field} collides with existing \
         node {existing_uid}"
    )]
    InsertedIdentityCollision {
        /// The patch's uid.
        patch_uid: String,
        /// The inserted node's uid.
        uid: String,
        /// The colliding identity field: `uid` or `label`.
        field: &'static str,
        /// The existing node the identity collides with.
        existing_uid: String,
    },
    /// The parser graph failed validation before any operation
    /// applied.
    #[error("curated patch {patch_uid} cannot apply: the parser graph is invalid")]
    PreGraphInvalid {
        /// The patch's uid.
        patch_uid: String,
        /// The source-graph validation failure. Boxed to keep the
        /// enum under clippy's `result_large_err` threshold.
        #[source]
        source: Box<SourceGraphError>,
    },
    /// The graph produced by the operations failed the complete
    /// source-graph validator.
    #[error("curated patch {patch_uid} produces an invalid post-patch graph")]
    InvalidPostGraph {
        /// The patch's uid.
        patch_uid: String,
        /// The source-graph validation failure. Boxed to keep the
        /// enum under clippy's `result_large_err` threshold.
        #[source]
        source: Box<SourceGraphError>,
    },
}
