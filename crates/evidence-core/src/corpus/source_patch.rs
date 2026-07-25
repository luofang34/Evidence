//! Digest-bound curated source-graph patch records (LLR-166,
//! LLR-167, LLR-168).
//!
//! A curated patch is a strict, deterministic committed artifact
//! that corrects parser output in one source revision's committed
//! structural graph without editing frozen source bytes and
//! without hiding parser loss (SYS-054). The patch binds exactly
//! one source-revision uid, the exact ingester recipe digest, the
//! exact verified input digest, and the exact pre-patch canonical
//! graph digest; a reviewed-content digest covers the canonical
//! patch intent — the ordered operations and all preconditions —
//! while author, rationale, and creation metadata stay outside
//! semantic identity. A patch is data in the committed corpus: it
//! never mutates the frozen source record or `sources.lock`.
//!
//! # Operations
//!
//! The operation language is the closed [`PatchOperation`] enum,
//! never generic JSON Patch — excluded capabilities are
//! unrepresentable rather than merely rejected (HLR-127):
//!
//! - `replace_content` — replace canonical text or label against
//!   an expected prior content digest;
//! - `reclassify` — change a node's kind against an explicit
//!   expected prior kind;
//! - `reparent` — move or reorder a node against an expected
//!   prior parent and ordinal;
//! - `insert` — insert a fully specified curated node under an
//!   expected parent;
//! - `remove` — remove a node only against an expected digest
//!   with an explicit [`ChildDisposition`].
//!
//! Every operation carries an ordinal fixing the canonical
//! application order, its target and precondition context, and a
//! deterministic result. Duplicate or conflicting operations,
//! implicit cascades, dangling targets, cross-document edits, and
//! ambiguous application order fail closed with typed errors.
//!
//! # Application
//!
//! [`apply_patch`] validates the parser graph first, verifies the
//! recipe, input, and pre-patch graph digest bindings, checks
//! every precondition before changing any node, applies operations
//! in canonical ordinal order against a working copy, and re-runs
//! the complete source-graph validator over the result (HLR-128).
//! Application is atomic: any failure discards the working copy
//! and the original graph is byte-identical. The returned
//! [`PatchApplication`] records the pre-patch digest, the patch
//! reviewed-content digest, the post-patch digest, and the
//! candidate graph — a separately inspectable plane, never merged
//! into the committed parser graph and carrying no
//! review-lifecycle or approval semantics. The approval-gated
//! effective committed graph is a later milestone's concern.
//!
//! Module map:
//!
//! - `error` — the flat [`SourcePatchError`] taxonomy every
//!   curated-patch failure reports through
//! - `records` — the strict `SourcePatchFile` serde schema, record
//!   validation, and the `load_source_patches_into` loader
//!   (LLR-166)
//! - `digest` — the reviewed-content encoding and the canonical
//!   source-graph digest (LLR-167)
//! - `apply` — the atomic precondition-checked candidate
//!   application contract (LLR-168)

use serde::Deserialize;

use super::digest::StructuralContentDigest;
use super::source_graph::SourceNodeKind;
use super::source_graph::locator::SourceLocator;

pub(super) mod apply;
pub(super) mod digest;
pub(super) mod error;
pub(super) mod records;

pub(crate) mod validate;

/// The closed curated-patch operation enum (LLR-166, HLR-127).
///
/// Serde snake_case with the tag named `op`; any other tag — a
/// generic JSON Patch verb, a source-record or lock mutation, a
/// review-lifecycle action — fails deserialization, so excluded
/// capabilities are unrepresentable rather than merely rejected.
/// Every operation carries its ordinal, its target and
/// precondition context, and a deterministic result.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum PatchOperation {
    /// Replace a node's canonical text or label, against the
    /// expected prior content digest.
    ReplaceContent {
        /// Application-order ordinal.
        ordinal: u32,
        /// The `snode_<UUIDv4>` node to change.
        target_uid: String,
        /// Expected prior content digest; stale fails closed.
        expected_content_sha256: StructuralContentDigest,
        /// Replacement canonical text; at least one of this and
        /// `new_label` must be present.
        #[serde(default)]
        new_canonical_text: Option<String>,
        /// Replacement label; non-blank when present.
        #[serde(default)]
        new_label: Option<String>,
    },
    /// Reclassify a node's kind, against the explicit expected
    /// prior kind.
    Reclassify {
        /// Application-order ordinal.
        ordinal: u32,
        /// The `snode_<UUIDv4>` node to reclassify.
        target_uid: String,
        /// Expected prior kind; stale fails closed.
        expected_kind: SourceNodeKind,
        /// The curated kind.
        new_kind: SourceNodeKind,
    },
    /// Reparent or reorder a node, against the expected prior
    /// parent and ordinal.
    Reparent {
        /// Application-order ordinal.
        ordinal: u32,
        /// The `snode_<UUIDv4>` node to move.
        target_uid: String,
        /// Expected prior parent; `None` names the root set.
        #[serde(default)]
        expected_parent_uid: Option<String>,
        /// Expected prior ordinal; stale fails closed.
        expected_ordinal: u32,
        /// Curated parent; `None` moves the node to the root set.
        /// Must resolve at application — dangling fails closed.
        #[serde(default)]
        new_parent_uid: Option<String>,
        /// Curated ordinal within the new sibling set.
        new_ordinal: u32,
    },
    /// Insert a fully specified curated node under an expected
    /// parent.
    Insert {
        /// Application-order ordinal.
        ordinal: u32,
        /// The parent the node is curated under; `None` inserts a
        /// root. Must resolve at application — dangling fails
        /// closed.
        #[serde(default)]
        expected_parent_uid: Option<String>,
        /// The complete curated node content.
        node: InsertedNodeSpec,
    },
    /// Remove a node, against the expected prior content digest,
    /// with an explicit child disposition — never an implicit
    /// cascade.
    Remove {
        /// Application-order ordinal.
        ordinal: u32,
        /// The `snode_<UUIDv4>` node to remove.
        target_uid: String,
        /// Expected prior content digest; stale fails closed.
        expected_digest: StructuralContentDigest,
        /// What happens to the node's children.
        child_disposition: ChildDisposition,
    },
}

/// The explicit child disposition of a `remove` operation
/// (LLR-166). Serializes snake_case on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChildDisposition {
    /// Promote the children to the removed node's parent, taking
    /// the removed node's position in their original order.
    ReparentChildren,
    /// Remove the node and its entire subtree — an explicit, not
    /// implicit, cascade.
    RemoveSubtree,
}

/// The fully specified content of a curated inserted node
/// (LLR-166). Everything a structural node needs except the
/// revision binding and parent link, which the patch and the
/// operation supply, and the digest fields, which application
/// recomputes from the kind, text, label, and ancestry.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InsertedNodeSpec {
    /// Permanent identity: `snode_<UUIDv4>`.
    pub uid: String,
    /// Closed structural kind.
    pub kind: SourceNodeKind,
    /// Position within the parent's sibling set.
    pub ordinal: u32,
    /// Optional human identity; non-blank when present.
    #[serde(default)]
    pub label: Option<String>,
    /// Canonical text under the kind's normalization contract.
    pub canonical_text: String,
    /// The one typed diagnostic locator.
    pub locator: SourceLocator,
}

impl PatchOperation {
    /// The operation's application-order ordinal.
    pub fn ordinal(&self) -> u32 {
        match self {
            PatchOperation::ReplaceContent { ordinal, .. }
            | PatchOperation::Reclassify { ordinal, .. }
            | PatchOperation::Reparent { ordinal, .. }
            | PatchOperation::Insert { ordinal, .. }
            | PatchOperation::Remove { ordinal, .. } => *ordinal,
        }
    }

    /// The operation's wire tag.
    pub fn op_tag(&self) -> &'static str {
        match self {
            PatchOperation::ReplaceContent { .. } => "replace_content",
            PatchOperation::Reclassify { .. } => "reclassify",
            PatchOperation::Reparent { .. } => "reparent",
            PatchOperation::Insert { .. } => "insert",
            PatchOperation::Remove { .. } => "remove",
        }
    }

    /// The operation's conflict identity: two operations of the
    /// same kind on the same conflict identity are a duplicate or
    /// conflicting pair. An insert's identity is its node uid.
    pub fn conflict_identity(&self) -> &str {
        match self {
            PatchOperation::ReplaceContent { target_uid, .. }
            | PatchOperation::Reclassify { target_uid, .. }
            | PatchOperation::Reparent { target_uid, .. }
            | PatchOperation::Remove { target_uid, .. } => target_uid,
            PatchOperation::Insert { node, .. } => &node.uid,
        }
    }
}

// Tests live in sibling files pulled in via `#[path]`: one module
// per TEST entry domain.
#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
#[path = "source_patch/tests.rs"]
mod tests;
#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
#[path = "source_patch/tests_apply/tests.rs"]
mod tests_apply;
#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
#[path = "source_patch/tests_support.rs"]
mod tests_support;
