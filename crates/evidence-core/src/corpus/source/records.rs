//! Corpus-native source-revision record schema (LLR-125).
//!
//! A source file is a strict, `schema_version`-gated TOML document
//! named by the corpus index's `sources` kind. Each record freezes
//! one revision of a source document: identity (`src_<UUIDv4>` uid,
//! human-readable revision id, stable document lineage key),
//! descriptive audit fields (title, media type, canonical location),
//! and a typed [`SourceMaterial`] state. Everything degenerate fails
//! closed with a typed [`SourceError`] naming the file path and the
//! record's id and uid: unknown fields, a newer file schema,
//! malformed uids or timestamps, a malformed media type, blank
//! required strings, an incomplete material or capture combination,
//! and an unsafe or non-canonical vendored path.
//!
//! `canonical_location` is opaque audit identity: preserved exactly,
//! never fetched or normalized as a URL. `retrieved_at` is audit
//! metadata, never ordering authority. Blank-string validation
//! rejects whitespace-only values and stores accepted text
//! untrimmed — validation never rewrites accepted input.

use std::path::Path;

use serde::Deserialize;

use super::super::graph::{CorpusGraph, Node, SourceCapture, SourceMaterial, SourceRevisionNode};
use super::super::records::validate_native_uid;
use super::SOURCE_UID_PREFIX;
use super::error::{SourceError, VendoredPathRule};

/// On-disk shape of a source-revision record file. Strict: unknown
/// fields are a parse error.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceFile {
    /// File schema version; newer than supported refuses to load.
    pub schema_version: u32,
    /// The source-revision records in the file.
    #[serde(default)]
    pub sources: Vec<SourceRevisionRecord>,
}

/// One frozen source-revision record. Strict: unknown fields are a
/// parse error. The material state and capture values deserialize
/// through their internally tagged, unknown-field-denying types, so
/// an incomplete or impossible combination — a vendored capture
/// missing its path, an unavailable state carrying a digest — fails
/// deserialization rather than loading as a valid record.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceRevisionRecord {
    /// Permanent identity: `src_<UUIDv4>`.
    pub uid: String,
    /// Human-readable revision identifier; non-empty.
    pub id: String,
    /// Stable lineage key grouping revisions of one logical
    /// document; non-blank.
    pub document_key: String,
    /// One-line title; non-blank.
    pub title: String,
    /// RFC 6838 `type/subtype` media type.
    pub media_type: String,
    /// Canonical location; non-blank. Opaque audit identity,
    /// preserved exactly.
    pub canonical_location: String,
    /// Typed material state of the revision.
    pub material: SourceMaterial,
}

/// Parse the source file at `path` and insert its records into
/// `graph`.
///
/// # Errors
///
/// Fails closed on unreadable/malformed input, a newer
/// `schema_version`, any invalid record field (naming the file path
/// and the record's id and uid), or a graph identity collision.
pub(crate) fn load_sources_into(path: &Path, graph: &mut CorpusGraph) -> Result<(), SourceError> {
    let raw = std::fs::read_to_string(path).map_err(|source| SourceError::RecordRead {
        path: path.to_path_buf(),
        source,
    })?;
    let file: SourceFile = toml::from_str(&raw).map_err(|source| SourceError::RecordParse {
        path: path.to_path_buf(),
        source,
    })?;
    if file.schema_version > SUPPORTED_SOURCE_SCHEMA {
        return Err(SourceError::RecordSchemaTooNew {
            path: path.to_path_buf(),
            found: file.schema_version,
            supported: SUPPORTED_SOURCE_SCHEMA,
        });
    }
    for record in file.sources {
        validate_record(path, &record)?;
        graph
            .insert(Node::SourceRevision(SourceRevisionNode {
                uid: record.uid,
                id: record.id,
                document_key: record.document_key,
                title: record.title,
                media_type: record.media_type,
                canonical_location: record.canonical_location,
                material: record.material,
                edges: Vec::new(),
            }))
            .map_err(SourceError::from_insert)?;
    }
    Ok(())
}

/// Highest source-file schema version this tool loads.
pub const SUPPORTED_SOURCE_SCHEMA: u32 = 1;

/// Validate one record's fields in declaration order; the first
/// failure wins, so error precedence is deterministic (LLR-125).
/// The digest inside an available material state is validated at
/// deserialization, before this pass runs.
fn validate_record(path: &Path, record: &SourceRevisionRecord) -> Result<(), SourceError> {
    validate_source_uid(&record.uid)?;
    if record.id.trim().is_empty() {
        return Err(SourceError::SourceHumanId {
            path: path.to_path_buf(),
            uid: record.uid.clone(),
        });
    }
    if record.document_key.trim().is_empty() {
        return Err(SourceError::SourceDocumentKey {
            path: path.to_path_buf(),
            uid: record.uid.clone(),
            id: record.id.clone(),
        });
    }
    if record.title.trim().is_empty() {
        return Err(SourceError::SourceTitle {
            path: path.to_path_buf(),
            uid: record.uid.clone(),
            id: record.id.clone(),
        });
    }
    if !is_valid_media_type(&record.media_type) {
        return Err(SourceError::SourceMediaType {
            path: path.to_path_buf(),
            uid: record.uid.clone(),
            id: record.id.clone(),
            value: record.media_type.clone(),
        });
    }
    if record.canonical_location.trim().is_empty() {
        return Err(SourceError::SourceCanonicalLocation {
            path: path.to_path_buf(),
            uid: record.uid.clone(),
            id: record.id.clone(),
        });
    }
    validate_material(path, record)?;
    Ok(())
}

/// Validate the typed material state: timestamp shape on available
/// material, non-blank reason on unavailable material, and the
/// per-capture field rules.
fn validate_material(path: &Path, record: &SourceRevisionRecord) -> Result<(), SourceError> {
    match &record.material {
        SourceMaterial::Available {
            retrieved_at,
            capture,
            ..
        } => {
            if chrono::DateTime::parse_from_rfc3339(retrieved_at).is_err() {
                return Err(SourceError::SourceTimestamp {
                    path: path.to_path_buf(),
                    uid: record.uid.clone(),
                    id: record.id.clone(),
                    value: retrieved_at.clone(),
                });
            }
            match capture {
                SourceCapture::Vendored { path: wire_path } => {
                    validate_vendored_wire_path(wire_path).map_err(|rule| {
                        SourceError::SourceVendoredPath {
                            path: path.to_path_buf(),
                            uid: record.uid.clone(),
                            id: record.id.clone(),
                            value: wire_path.clone(),
                            rule,
                        }
                    })?;
                }
                SourceCapture::HashOnly {} => {}
                SourceCapture::ExternalControlled {
                    system,
                    immutable_id,
                } => {
                    if system.trim().is_empty() {
                        return Err(SourceError::SourceCaptureSystem {
                            path: path.to_path_buf(),
                            uid: record.uid.clone(),
                            id: record.id.clone(),
                        });
                    }
                    if immutable_id.trim().is_empty() {
                        return Err(SourceError::SourceCaptureImmutableId {
                            path: path.to_path_buf(),
                            uid: record.uid.clone(),
                            id: record.id.clone(),
                        });
                    }
                }
            }
        }
        SourceMaterial::Unavailable { reason } => {
            if reason.trim().is_empty() {
                return Err(SourceError::SourceReason {
                    path: path.to_path_buf(),
                    uid: record.uid.clone(),
                    id: record.id.clone(),
                });
            }
        }
    }
    Ok(())
}

/// A source uid is `src_` followed by an RFC 9562 UUIDv4 — the
/// corpus-native uid contract with the source kind's prefix
/// (LLR-125).
fn validate_source_uid(uid: &str) -> Result<(), SourceError> {
    validate_native_uid(
        uid,
        SOURCE_UID_PREFIX,
        |uid, expected| SourceError::NativeUidPrefix { uid, expected },
        |uid| SourceError::NativeUidUuidV4 { uid },
    )
}

/// The accepted media type grammar: RFC 6838 `type/subtype` token
/// form. Both names are one or more characters, start with an
/// ASCII letter or digit, and continue with ASCII letters, digits,
/// or the restricted-name punctuation `!#$&-^_.+`; matching is
/// case-insensitive in the RFC, so uppercase is accepted and the
/// value is stored exactly as written. Empty names, whitespace,
/// missing or repeated `/`, and any other character are rejected.
fn is_valid_media_type(value: &str) -> bool {
    let Some((media_type, subtype)) = value.split_once('/') else {
        return false;
    };
    let valid_name = |name: &str| {
        !name.is_empty()
            && name
                .bytes()
                .next()
                .is_some_and(|b| b.is_ascii_alphanumeric())
            && name.bytes().all(|b| {
                b.is_ascii_alphanumeric()
                    || matches!(
                        b,
                        b'!' | b'#' | b'$' | b'&' | b'-' | b'^' | b'_' | b'.' | b'+'
                    )
            })
    };
    valid_name(media_type) && valid_name(subtype)
}

/// Validate a vendored capture path as the canonical `/`-separated
/// relative wire form beneath the fixed `sources/` payload root
/// (LLR-125): reject empty paths, absolute paths, drive or UNC
/// prefixes, backslashes, and empty, `.`, or `..` components.
///
/// The check is lexical only — it never touches the filesystem.
/// Containment, regular-file, and symlink checks belong to the
/// acquisition layer above this one.
pub(crate) fn validate_vendored_wire_path(value: &str) -> Result<(), VendoredPathRule> {
    if value.is_empty() {
        return Err(VendoredPathRule::Empty);
    }
    if value.starts_with('/') {
        return Err(VendoredPathRule::Absolute);
    }
    if value.starts_with("\\\\") {
        return Err(VendoredPathRule::UncPrefix);
    }
    if value.len() >= 2 && value.as_bytes()[1] == b':' && value.as_bytes()[0].is_ascii_alphabetic()
    {
        return Err(VendoredPathRule::DrivePrefix);
    }
    if value.contains('\\') {
        return Err(VendoredPathRule::Backslash);
    }
    for component in value.split('/') {
        match component {
            "" => return Err(VendoredPathRule::EmptyComponent),
            "." => return Err(VendoredPathRule::DotComponent),
            ".." => return Err(VendoredPathRule::ParentComponent),
            _ => {}
        }
    }
    Ok(())
}
