//! Corpus-native source-graph record schema (LLR-152).
//!
//! A source-graph file is a strict, `schema_version`-gated TOML
//! document named by the corpus index's `source_graphs` kind.
//! Each record freezes one structural source node as parsed from
//! one frozen source revision: permanent identity
//! (`snode_<UUIDv4>` uid), revision binding (`src_<UUIDv4>`), an
//! optional in-revision parent, the closed structural kind, the
//! sibling ordinal, an optional label, canonical text, the
//! content digest, the structural fingerprint, and one typed
//! locator. Everything degenerate fails closed with a typed
//! [`SourceGraphError`] naming the file path and the record uid:
//! unknown fields, a newer file schema, malformed uids, a blank
//! label, and invalid locator fields. Malformed digests and
//! unsafe locator paths fail at deserialization through the
//! validating digest and path newtypes.
//!
//! Blank-string validation rejects whitespace-only labels and
//! stores accepted text untrimmed — validation never rewrites
//! accepted input. Structural invariants (parents, cycles,
//! ordinals, digest recomputation) are graph-validation concerns,
//! not record-loading concerns; loading enforces only per-record
//! field validity and per-revision identity uniqueness.

use std::path::Path;

use serde::Deserialize;

use super::super::digest::StructuralContentDigest;
use super::super::graph::CorpusGraph;
use super::super::records::validate_native_uid;
use super::super::source::SOURCE_UID_PREFIX;
use super::error::SourceGraphError;
use super::locator::SourceLocator;
use super::{SourceNode, SourceNodeKind};

/// Highest source-graph-file schema version this tool loads.
pub const SUPPORTED_SOURCE_GRAPH_SCHEMA: u32 = 1;

/// Typed uid prefix for corpus-native structural source nodes
/// (LLR-152).
pub const SNODE_UID_PREFIX: &str = "snode_";

/// On-disk shape of a source-graph record file. Strict: unknown
/// fields are a parse error.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceGraphFile {
    /// File schema version; newer than supported refuses to load.
    pub schema_version: u32,
    /// The structural source-node records in the file.
    #[serde(default)]
    pub nodes: Vec<SourceNodeRecord>,
}

/// One structural source-node record. Strict: unknown fields are
/// a parse error. The kind and locator deserialize through their
/// closed, unknown-field-denying types, and the digest fields
/// through the validating structural digest contract, so an
/// impossible combination — a mixed-format locator, a malformed
/// digest, an unknown kind — fails deserialization rather than
/// loading as a valid record.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceNodeRecord {
    /// Permanent identity: `snode_<UUIDv4>`.
    pub uid: String,
    /// The `src_<UUIDv4>` revision this node was parsed from.
    pub source_revision_uid: String,
    /// Optional `snode_<UUIDv4>` parent inside the same revision.
    #[serde(default)]
    pub parent_uid: Option<String>,
    /// Closed structural kind.
    pub kind: SourceNodeKind,
    /// Position within the parent's sibling set.
    pub ordinal: u32,
    /// Optional human identity; non-blank when present.
    #[serde(default)]
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

/// Parse the source-graph file at `path` and insert its records
/// into the corpus graph's per-revision source graphs.
///
/// # Errors
///
/// Fails closed on unreadable/malformed input, a newer
/// `schema_version`, any invalid record field (naming the file
/// path and the record uid), or a per-revision identity
/// collision.
pub(crate) fn load_source_graphs_into(
    path: &Path,
    graph: &mut CorpusGraph,
) -> Result<(), SourceGraphError> {
    let raw = std::fs::read_to_string(path).map_err(|source| SourceGraphError::RecordRead {
        path: path.to_path_buf(),
        source,
    })?;
    let file: SourceGraphFile =
        toml::from_str(&raw).map_err(|source| SourceGraphError::RecordParse {
            path: path.to_path_buf(),
            source,
        })?;
    if file.schema_version > SUPPORTED_SOURCE_GRAPH_SCHEMA {
        return Err(SourceGraphError::RecordSchemaTooNew {
            path: path.to_path_buf(),
            found: file.schema_version,
            supported: SUPPORTED_SOURCE_GRAPH_SCHEMA,
        });
    }
    for record in file.nodes {
        validate_record(path, &record)?;
        graph.insert_source_node(SourceNode {
            uid: record.uid,
            source_revision_uid: record.source_revision_uid,
            parent_uid: record.parent_uid,
            kind: record.kind,
            ordinal: record.ordinal,
            label: record.label,
            canonical_text: record.canonical_text,
            content_sha256: record.content_sha256,
            fingerprint: record.fingerprint,
            locator: record.locator,
        })?;
    }
    Ok(())
}

/// Validate one record's fields in declaration order; the first
/// failure wins, so error precedence is deterministic (LLR-152).
/// The digest fields and the locator path validate at
/// deserialization, before this pass runs.
fn validate_record(path: &Path, record: &SourceNodeRecord) -> Result<(), SourceGraphError> {
    validate_snode_uid(&record.uid)?;
    validate_native_uid(
        &record.source_revision_uid,
        SOURCE_UID_PREFIX,
        |uid, expected| SourceGraphError::NativeUidPrefix { uid, expected },
        |uid| SourceGraphError::NativeUidUuidV4 { uid },
    )?;
    if let Some(parent) = &record.parent_uid {
        validate_snode_uid(parent)?;
    }
    if let Some(label) = &record.label {
        if label.trim().is_empty() {
            return Err(SourceGraphError::NodeLabel {
                path: path.to_path_buf(),
                uid: record.uid.clone(),
            });
        }
    }
    record
        .locator
        .validate()
        .map_err(|err| SourceGraphError::InvalidLocatorField {
            path: path.to_path_buf(),
            node_uid: record.uid.clone(),
            field: err.field,
            value: err.value,
            rule: err.rule,
        })?;
    Ok(())
}

/// A structural node uid is `snode_` followed by an RFC 9562
/// UUIDv4 — the corpus-native uid contract with the source-graph
/// kind's prefix (LLR-152).
fn validate_snode_uid(uid: &str) -> Result<(), SourceGraphError> {
    validate_native_uid(
        uid,
        SNODE_UID_PREFIX,
        |uid, expected| SourceGraphError::NativeUidPrefix { uid, expected },
        |uid| SourceGraphError::NativeUidUuidV4 { uid },
    )
}
