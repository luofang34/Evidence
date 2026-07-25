//! The curated-patch reviewed-content encoding and the canonical
//! source-graph digest (LLR-167).
//!
//! [`reviewed_content_bytes`] encodes the canonical patch intent —
//! everything a reviewer approves — under the domain/version tag
//! `evidence/curated-patch/v1` and the length-prefixed framing of
//! the source-node normalization encodings, so no field-boundary
//! collision is possible and the encoding is disjoint from every
//! other corpus encoding:
//!
//! ```text
//! b"evidence/curated-patch/v1" || 0x00
//! || str(source_revision_uid)
//! || str(recipe_digest hex) || str(input_digest hex)
//! || str(pre_patch_graph_digest hex)
//! || u64_be(operations.len())
//! || for each operation in canonical ordinal order:
//!      str(op wire tag) || u64_be(ordinal) || per-variant fields
//! ```
//!
//! Per-variant fields, in declaration order:
//!
//! - `replace_content`: `str(target_uid)` ||
//!   `str(expected_content_sha256 hex)` || `opt(new_canonical_text)`
//!   || `opt(new_label)`
//! - `reclassify`: `str(target_uid)` || `str(expected_kind)` ||
//!   `str(new_kind)`
//! - `reparent`: `str(target_uid)` || `opt(expected_parent_uid)` ||
//!   `u64_be(expected_ordinal)` || `opt(new_parent_uid)` ||
//!   `u64_be(new_ordinal)`
//! - `insert`: `opt(expected_parent_uid)` || `str(node.uid)` ||
//!   `str(node.kind)` || `u64_be(node.ordinal)` || `opt(node.label)`
//!   || `str(node.canonical_text)` || `str(node.locator rendering)`
//! - `remove`: `str(target_uid)` || `str(expected_digest hex)` ||
//!   `str(child_disposition wire name)`
//!
//! where `str(s)` is `u64_be(byte length of s) || s`'s exact UTF-8
//! bytes, `opt(o)` is the all-ones sentinel `0xFFFFFFFFFFFFFFFF` for
//! `None` or `str(v)` for `Some(v)`, and the inserted node's
//! locator renders through the canonical source-graph locator
//! rendering (`render_locator_fields`), so one contract pins both
//! surfaces. Operations encode sorted by ordinal: the ordinal,
//! never the record file layout, fixes the canonical order, so
//! reordered record files digest identically.
//!
//! The patch uid, the human id, author, rationale, and creation
//! metadata never enter the encoding — they are audit and identity
//! metadata outside semantic identity; the four bindings and every
//! precondition of every operation are inside it.
//! [`reviewed_content_digest`] is the validated structural digest
//! over those bytes. Changing this contract requires a new
//! encoding version, never a silent change of existing digests.
//!
//! [`source_graph_digest`] is the canonical digest of one source
//! revision's committed graph: SHA-256 over the domain/version tag
//! `evidence/source-graph-digest/v1`, a `0x00` separator, and the
//! canonical rendering of the graph's nodes in uid order
//! (`render_sorted_nodes`). Equivalent layouts digest identically;
//! any structural difference — a changed text, label, kind,
//! parent, ordinal, or locator — moves the digest.

use super::super::digest::StructuralContentDigest;
use super::super::source_graph::SourceGraph;
use super::super::source_graph::render::{render_locator_fields, render_sorted_nodes};
use super::records::SourcePatchRecord;
use super::{ChildDisposition, PatchOperation};

/// Domain/version tag prefixing the reviewed-content encoding.
const REVIEWED_DOMAIN_TAG: &[u8] = b"evidence/curated-patch/v1";

/// Domain/version tag prefixing the canonical source-graph digest.
const GRAPH_DOMAIN_TAG: &[u8] = b"evidence/source-graph-digest/v1";

/// `None` sentinel for optional fields: the all-ones `u64`, which
/// no real byte length can reach.
const NONE_SENTINEL: u64 = u64::MAX;

/// Encode `record`'s canonical patch intent in the reviewed-content
/// byte format pinned by the module docs. Pure and host-independent.
pub fn reviewed_content_bytes(record: &SourcePatchRecord) -> Vec<u8> {
    let mut operations = record.operations.clone();
    operations.sort_by_key(PatchOperation::ordinal);
    let mut bytes = Vec::new();
    bytes.extend_from_slice(REVIEWED_DOMAIN_TAG);
    bytes.push(0);
    push_str(&mut bytes, &record.source_revision_uid);
    push_str(&mut bytes, record.recipe_digest.as_str());
    push_str(&mut bytes, record.input_digest.as_str());
    push_str(&mut bytes, record.pre_patch_graph_digest.as_str());
    push_count(&mut bytes, operations.len());
    for operation in &operations {
        push_operation(&mut bytes, operation);
    }
    bytes
}

/// The reviewed-content digest of `record`: SHA-256 over
/// [`reviewed_content_bytes`], as the validated structural digest
/// domain (LLR-167).
pub fn reviewed_content_digest(record: &SourcePatchRecord) -> StructuralContentDigest {
    StructuralContentDigest::from_hasher_output(crate::hash::sha256(&reviewed_content_bytes(
        record,
    )))
}

/// The canonical digest of one source revision's committed graph
/// (LLR-167): SHA-256 over the domain-tagged canonical rendering
/// of the graph's nodes in uid order, pinned by the module docs.
/// Pure and host-independent.
pub fn source_graph_digest(graph: &SourceGraph) -> StructuralContentDigest {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(GRAPH_DOMAIN_TAG);
    bytes.push(0);
    bytes.extend_from_slice(&render_sorted_nodes(graph.nodes().collect()));
    StructuralContentDigest::from_hasher_output(crate::hash::sha256(&bytes))
}

/// Encode one operation: its wire tag, ordinal, and per-variant
/// fields in declaration order.
fn push_operation(bytes: &mut Vec<u8>, operation: &PatchOperation) {
    push_str(bytes, operation.op_tag());
    push_count(bytes, operation.ordinal() as usize);
    match operation {
        PatchOperation::ReplaceContent {
            target_uid,
            expected_content_sha256,
            new_canonical_text,
            new_label,
            ..
        } => {
            push_str(bytes, target_uid);
            push_str(bytes, expected_content_sha256.as_str());
            push_opt(bytes, new_canonical_text.as_deref());
            push_opt(bytes, new_label.as_deref());
        }
        PatchOperation::Reclassify {
            target_uid,
            expected_kind,
            new_kind,
            ..
        } => {
            push_str(bytes, target_uid);
            push_str(bytes, expected_kind.as_str());
            push_str(bytes, new_kind.as_str());
        }
        PatchOperation::Reparent {
            target_uid,
            expected_parent_uid,
            expected_ordinal,
            new_parent_uid,
            new_ordinal,
            ..
        } => {
            push_str(bytes, target_uid);
            push_opt(bytes, expected_parent_uid.as_deref());
            push_count(bytes, *expected_ordinal as usize);
            push_opt(bytes, new_parent_uid.as_deref());
            push_count(bytes, *new_ordinal as usize);
        }
        PatchOperation::Insert {
            expected_parent_uid,
            node,
            ..
        } => {
            push_opt(bytes, expected_parent_uid.as_deref());
            push_str(bytes, &node.uid);
            push_str(bytes, node.kind.as_str());
            push_count(bytes, node.ordinal as usize);
            push_opt(bytes, node.label.as_deref());
            push_str(bytes, &node.canonical_text);
            push_str(bytes, &render_locator_fields(&node.locator));
        }
        PatchOperation::Remove {
            target_uid,
            expected_digest,
            child_disposition,
            ..
        } => {
            push_str(bytes, target_uid);
            push_str(bytes, expected_digest.as_str());
            push_str(bytes, child_disposition_tag(*child_disposition));
        }
    }
}

/// The child disposition's snake_case wire name.
fn child_disposition_tag(disposition: ChildDisposition) -> &'static str {
    match disposition {
        ChildDisposition::ReparentChildren => "reparent_children",
        ChildDisposition::RemoveSubtree => "remove_subtree",
    }
}

/// `str(s)` framing: `u64_be` byte length, then the exact UTF-8
/// bytes.
fn push_str(out: &mut Vec<u8>, value: &str) {
    push_count(out, value.len());
    out.extend_from_slice(value.as_bytes());
}

/// `opt(o)` framing: the all-ones sentinel for `None`, `str(v)`
/// for `Some(v)`.
fn push_opt(out: &mut Vec<u8>, value: Option<&str>) {
    match value {
        Some(value) => push_str(out, value),
        None => out.extend_from_slice(&NONE_SENTINEL.to_be_bytes()),
    }
}

fn push_count(out: &mut Vec<u8>, count: usize) {
    out.extend_from_slice(&(count as u64).to_be_bytes());
}
