//! Corpus-native curated-patch record schema (LLR-166).
//!
//! A curated-patch file is a strict, `schema_version`-gated TOML
//! document named by the corpus index's `source_patches` kind.
//! Each record freezes one curated patch against one source
//! revision's committed parser graph: permanent identity
//! (`patch_<UUIDv4>` uid), a human id unique within the
//! curated-patch kind, the four bindings (source-revision uid,
//! ingester recipe digest, verified input digest, pre-patch
//! canonical graph digest), the reviewed-content digest over the
//! canonical patch intent, the author, rationale, and RFC 3339
//! creation metadata that stay outside semantic identity, and the
//! ordered operations. Everything degenerate fails closed with a
//! typed [`SourcePatchError`] naming the file path and the patch
//! uid: unknown fields, a newer file schema, malformed uids,
//! blank metadata, an unknown operation tag (the operation enum
//! is closed — excluded capabilities are unrepresentable),
//! duplicate or conflicting operations, and a reviewed-content
//! digest that does not recompute from the bindings and ordered
//! operations. Malformed digests and unsafe locator paths fail at
//! deserialization through the validating digest and path
//! newtypes.
//!
//! Operations load sorted by ordinal, so a record file whose
//! operation blocks are reordered produces the identical patch —
//! the ordinal, never the file layout, fixes the canonical
//! application order. Blank-string validation rejects
//! whitespace-only human ids, metadata, and labels; validation
//! never rewrites accepted input.

use std::path::Path;

use serde::Deserialize;

use super::super::digest::StructuralContentDigest;
use super::super::graph::CorpusGraph;
use super::super::records::validate_native_uid;
use super::super::source::SOURCE_UID_PREFIX;
use super::super::source_graph::records::SNODE_UID_PREFIX;
use super::PatchOperation;
use super::digest;
use super::error::SourcePatchError;

/// Highest curated-patch-file schema version this tool loads.
pub const SUPPORTED_SOURCE_PATCH_SCHEMA: u32 = 1;

/// Typed uid prefix for corpus-native curated patches (LLR-166).
pub const PATCH_UID_PREFIX: &str = "patch_";

/// On-disk shape of a curated-patch record file. Strict: unknown
/// fields are a parse error.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourcePatchFile {
    /// File schema version; newer than supported refuses to load.
    pub schema_version: u32,
    /// The curated-patch record.
    pub patch: SourcePatchRecord,
}

/// One curated-patch record (LLR-166). Strict: unknown fields are
/// a parse error. The digest fields deserialize through the
/// validating structural digest contract and the operations
/// through the closed [`PatchOperation`] enum, so an impossible
/// combination — a malformed digest, an unknown operation tag, an
/// unknown kind — fails deserialization rather than loading as a
/// valid record.
///
/// The operations are sorted by ordinal at load: the ordinal,
/// never the file layout, fixes the canonical application order.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourcePatchRecord {
    /// Permanent identity: `patch_<UUIDv4>`.
    pub uid: String,
    /// Human identity; unique within the curated-patch kind.
    pub human_id: String,
    /// The `src_<UUIDv4>` revision this patch corrects.
    pub source_revision_uid: String,
    /// The exact ingester recipe digest the patch was curated
    /// against.
    pub recipe_digest: StructuralContentDigest,
    /// The exact verified input digest the patch was curated
    /// against.
    pub input_digest: StructuralContentDigest,
    /// The exact canonical digest of the pre-patch parser graph.
    pub pre_patch_graph_digest: StructuralContentDigest,
    /// Digest over the canonical patch intent: the bindings, the
    /// ordered operations, and all preconditions.
    pub reviewed_content_digest: StructuralContentDigest,
    /// Author audit identity. Metadata only — outside semantic
    /// identity and never accepted as proof of authority.
    pub author: String,
    /// Curator's rationale. Metadata only — outside semantic
    /// identity.
    pub rationale: String,
    /// RFC 3339 creation timestamp. Metadata only; never ordering
    /// authority.
    pub created_at: String,
    /// The ordered operations; unique ordinals, sorted at load.
    #[serde(default)]
    pub operations: Vec<PatchOperation>,
}

/// Parse and validate the curated-patch record in `raw`, naming
/// `path` in any error. Pure: no I/O. The returned record's
/// operations are sorted by ordinal.
///
/// # Errors
///
/// Fails closed on malformed input, a newer `schema_version`, any
/// invalid record field, duplicate or conflicting operations, or
/// a reviewed-content digest that does not recompute.
pub fn parse_source_patch(path: &Path, raw: &str) -> Result<SourcePatchRecord, SourcePatchError> {
    let file: SourcePatchFile =
        toml::from_str(raw).map_err(|source| SourcePatchError::RecordParse {
            path: path.to_path_buf(),
            source,
        })?;
    if file.schema_version > SUPPORTED_SOURCE_PATCH_SCHEMA {
        return Err(SourcePatchError::RecordSchemaTooNew {
            path: path.to_path_buf(),
            found: file.schema_version,
            supported: SUPPORTED_SOURCE_PATCH_SCHEMA,
        });
    }
    let mut record = file.patch;
    validate_record(path, &mut record)?;
    Ok(record)
}

/// Parse the curated-patch file at `path` and insert its record
/// into the corpus graph's patch plane.
///
/// # Errors
///
/// Fails closed on unreadable/malformed input, a newer
/// `schema_version`, any invalid record field (naming the file
/// path and the patch uid), or a patch identity collision.
pub(crate) fn load_source_patches_into(
    path: &Path,
    graph: &mut CorpusGraph,
) -> Result<(), SourcePatchError> {
    let raw = std::fs::read_to_string(path).map_err(|source| SourcePatchError::RecordRead {
        path: path.to_path_buf(),
        source,
    })?;
    let record = parse_source_patch(path, &raw)?;
    graph.insert_source_patch(record)
}

/// Validate a record's fields in declaration order; the first
/// failure wins, so error precedence is deterministic (LLR-166).
/// The digest fields and the insert locator path validate at
/// deserialization, before this pass runs. On success the
/// operations are sorted by ordinal.
fn validate_record(path: &Path, record: &mut SourcePatchRecord) -> Result<(), SourcePatchError> {
    validate_patch_uid(&record.uid)?;
    for (field, value) in [
        ("human_id", &record.human_id),
        ("author", &record.author),
        ("rationale", &record.rationale),
    ] {
        if value.trim().is_empty() {
            return Err(SourcePatchError::BlankField {
                path: path.to_path_buf(),
                uid: record.uid.clone(),
                field,
            });
        }
    }
    validate_native_uid(
        &record.source_revision_uid,
        SOURCE_UID_PREFIX,
        |uid, expected| SourcePatchError::NativeUidPrefix { uid, expected },
        |uid| SourcePatchError::NativeUidUuidV4 { uid },
    )?;
    if chrono::DateTime::parse_from_rfc3339(&record.created_at).is_err() {
        return Err(SourcePatchError::PatchTimestamp {
            path: path.to_path_buf(),
            uid: record.uid.clone(),
            value: record.created_at.clone(),
        });
    }
    validate_operations(path, record)?;
    let recomputed = digest::reviewed_content_digest(record);
    if recomputed != record.reviewed_content_digest {
        return Err(SourcePatchError::ReviewedContentDigestMismatch {
            path: path.to_path_buf(),
            uid: record.uid.clone(),
            expected: recomputed.as_str().to_string(),
            actual: record.reviewed_content_digest.as_str().to_string(),
        });
    }
    Ok(())
}

/// Validate the operation list: non-empty, per-operation field
/// validity in declaration order, unique ordinals, and no
/// duplicate or conflicting operation pairs; then sort by
/// ordinal so the ordinal, never the file layout, fixes the
/// canonical application order.
fn validate_operations(
    path: &Path,
    record: &mut SourcePatchRecord,
) -> Result<(), SourcePatchError> {
    if record.operations.is_empty() {
        return Err(SourcePatchError::EmptyOperations {
            path: path.to_path_buf(),
            uid: record.uid.clone(),
        });
    }
    for operation in &record.operations {
        validate_operation(path, &record.uid, operation)?;
    }
    let mut seen: std::collections::BTreeMap<u32, ()> = std::collections::BTreeMap::new();
    for operation in &record.operations {
        if seen.insert(operation.ordinal(), ()).is_some() {
            return Err(SourcePatchError::DuplicateOperationOrdinal {
                path: path.to_path_buf(),
                uid: record.uid.clone(),
                ordinal: operation.ordinal(),
            });
        }
    }
    let mut claims: std::collections::BTreeMap<(&'static str, &str), ()> =
        std::collections::BTreeMap::new();
    for operation in &record.operations {
        let claim = (operation.op_tag(), operation.conflict_identity());
        if claims.insert(claim, ()).is_some() {
            return Err(SourcePatchError::ConflictingOperation {
                path: path.to_path_buf(),
                uid: record.uid.clone(),
                op: operation.op_tag(),
                target_uid: operation.conflict_identity().to_string(),
            });
        }
    }
    record.operations.sort_by_key(PatchOperation::ordinal);
    Ok(())
}

/// Validate one operation's fields in declaration order.
fn validate_operation(
    path: &Path,
    patch_uid: &str,
    operation: &PatchOperation,
) -> Result<(), SourcePatchError> {
    match operation {
        PatchOperation::ReplaceContent {
            ordinal,
            target_uid,
            new_canonical_text,
            new_label,
            ..
        } => {
            validate_snode_uid(target_uid)?;
            if new_canonical_text.is_none() && new_label.is_none() {
                return Err(SourcePatchError::IncompleteReplaceContent {
                    path: path.to_path_buf(),
                    uid: patch_uid.to_string(),
                    ordinal: *ordinal,
                });
            }
            validate_optional_label(path, patch_uid, new_label.as_ref())?;
        }
        PatchOperation::Reclassify { target_uid, .. } => validate_snode_uid(target_uid)?,
        PatchOperation::Reparent {
            target_uid,
            expected_parent_uid,
            new_parent_uid,
            ..
        } => {
            validate_snode_uid(target_uid)?;
            validate_optional_snode_uid(expected_parent_uid.as_ref())?;
            validate_optional_snode_uid(new_parent_uid.as_ref())?;
        }
        PatchOperation::Insert {
            expected_parent_uid,
            node,
            ..
        } => {
            validate_optional_snode_uid(expected_parent_uid.as_ref())?;
            validate_snode_uid(&node.uid)?;
            validate_optional_label(path, patch_uid, node.label.as_ref())?;
            node.locator
                .validate()
                .map_err(|err| SourcePatchError::InvalidLocatorField {
                    path: path.to_path_buf(),
                    uid: patch_uid.to_string(),
                    field: err.field,
                    value: err.value,
                    rule: err.rule,
                })?;
        }
        PatchOperation::Remove { target_uid, .. } => validate_snode_uid(target_uid)?,
    }
    Ok(())
}

/// A present label must carry content (LLR-166).
fn validate_optional_label(
    path: &Path,
    patch_uid: &str,
    label: Option<&String>,
) -> Result<(), SourcePatchError> {
    if let Some(label) = label {
        if label.trim().is_empty() {
            return Err(SourcePatchError::BlankField {
                path: path.to_path_buf(),
                uid: patch_uid.to_string(),
                field: "label",
            });
        }
    }
    Ok(())
}

fn validate_optional_snode_uid(uid: Option<&String>) -> Result<(), SourcePatchError> {
    match uid {
        Some(uid) => validate_snode_uid(uid),
        None => Ok(()),
    }
}

/// A structural node uid is `snode_` followed by an RFC 9562
/// UUIDv4 — the corpus-native uid contract with the source-graph
/// kind's prefix (LLR-166).
fn validate_snode_uid(uid: &str) -> Result<(), SourcePatchError> {
    validate_native_uid(
        uid,
        SNODE_UID_PREFIX,
        |uid, expected| SourcePatchError::NativeUidPrefix { uid, expected },
        |uid| SourcePatchError::NativeUidUuidV4 { uid },
    )
}

/// A curated-patch uid is `patch_` followed by an RFC 9562 UUIDv4
/// (LLR-166).
fn validate_patch_uid(uid: &str) -> Result<(), SourcePatchError> {
    validate_native_uid(
        uid,
        PATCH_UID_PREFIX,
        |uid, expected| SourcePatchError::NativeUidPrefix { uid, expected },
        |uid| SourcePatchError::NativeUidUuidV4 { uid },
    )
}
