//! Corpus-native human review decision records (LLR-114).
//!
//! A review file is a strict, `schema_version`-gated TOML document
//! named by the corpus index's `reviews` kind. Each record binds one
//! reviewer's decision to a requirement uid and the exact
//! reviewed-content digest — the typed [`ReviewContentDigest`] of the
//! canonical v1 projection (LLR-111). Everything degenerate fails
//! closed with a typed [`ReviewError`] naming the file path and the
//! record's id and uid: unknown fields, a newer file or content
//! schema, malformed uids, digests, or timestamps, an empty reviewer
//! identity or human id, and a rejection without a rationale.
//!
//! `reviewer` and `reviewed_at` are audit metadata: the reviewer
//! identity is recorded, never accepted as proof that a caller is
//! human, and the timestamp never chooses an effective decision.
//! Review file layout and load order are non-semantic.

use std::path::Path;

use serde::Deserialize;

use super::digest::ReviewContentDigest;
use super::graph::{CorpusGraph, EdgeKind, Node, ReviewDecision, ReviewNode};
use super::records::{REQUIREMENT_UID_PREFIX, validate_native_uid};
use error::ReviewError;

pub(super) mod error;

/// Highest review-file schema version this tool loads.
pub const SUPPORTED_REVIEW_SCHEMA: u32 = 1;

/// The review-content projection version a record's digest must
/// cover (LLR-111).
pub const SUPPORTED_REVIEW_CONTENT_SCHEMA: u32 = 1;

/// Typed uid prefix for corpus-native review records (LLR-114).
pub const REVIEW_UID_PREFIX: &str = "rev_";

/// On-disk shape of a review record file. Strict: unknown fields
/// are a parse error.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewFile {
    /// File schema version; newer than supported refuses to load.
    pub schema_version: u32,
    /// The review records in the file.
    #[serde(default)]
    pub reviews: Vec<ReviewRecord>,
}

/// One human review decision record. Strict: unknown fields are a
/// parse error.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewRecord {
    /// Permanent identity: `rev_<UUIDv4>`.
    pub uid: String,
    /// Human-readable identifier (e.g. `REV-001`); non-empty.
    pub id: String,
    /// Uid of the reviewed requirement: `req_<UUIDv4>`.
    pub requirement_uid: String,
    /// Review-content projection version the digest covers; must
    /// equal [`SUPPORTED_REVIEW_CONTENT_SCHEMA`].
    pub content_schema: u32,
    /// Exact digest of the reviewed canonical content; serde
    /// deserialization validates length and character set.
    pub reviewed_content_sha256: ReviewContentDigest,
    /// `approve` or `reject`.
    pub decision: ReviewDecision,
    /// Organization-stable reviewer identity (audit metadata);
    /// non-empty after trimming.
    pub reviewer: String,
    /// RFC 3339 review timestamp (audit metadata only).
    pub reviewed_at: String,
    /// Why the decision was made; required and non-empty for
    /// rejections, optional for approvals.
    #[serde(default)]
    pub rationale: Option<String>,
    /// Optional `rev_<UUIDv4>` of the predecessor review this
    /// record corrects.
    #[serde(default)]
    pub supersedes_review_uid: Option<String>,
}

/// Parse the review file at `path` and insert its records into
/// `graph`.
///
/// # Errors
///
/// Fails closed on unreadable/malformed input, a newer
/// `schema_version`, any invalid record field (naming the file path
/// and the record's id and uid), or a graph identity collision.
pub(super) fn load_reviews_into(path: &Path, graph: &mut CorpusGraph) -> Result<(), ReviewError> {
    let raw = std::fs::read_to_string(path).map_err(|source| ReviewError::RecordRead {
        path: path.to_path_buf(),
        source,
    })?;
    let file: ReviewFile = toml::from_str(&raw).map_err(|source| ReviewError::RecordParse {
        path: path.to_path_buf(),
        source,
    })?;
    if file.schema_version > SUPPORTED_REVIEW_SCHEMA {
        return Err(ReviewError::RecordSchemaTooNew {
            path: path.to_path_buf(),
            found: file.schema_version,
            supported: SUPPORTED_REVIEW_SCHEMA,
        });
    }
    for record in file.reviews {
        validate_record(path, &record)?;
        let mut edges = vec![(EdgeKind::Reviews, record.requirement_uid.clone())];
        if let Some(target) = &record.supersedes_review_uid {
            edges.push((EdgeKind::Supersedes, target.clone()));
        }
        graph
            .insert(Node::Review(ReviewNode {
                uid: record.uid,
                id: record.id,
                requirement_uid: record.requirement_uid,
                content_schema: record.content_schema,
                reviewed_content_sha256: record.reviewed_content_sha256,
                decision: record.decision,
                reviewer: record.reviewer,
                reviewed_at: record.reviewed_at,
                rationale: record.rationale,
                edges,
            }))
            .map_err(ReviewError::from_insert)?;
    }
    Ok(())
}

/// Validate one record's fields in declaration order; the first
/// failure wins, so error precedence is deterministic (LLR-114).
fn validate_record(path: &Path, record: &ReviewRecord) -> Result<(), ReviewError> {
    validate_review_uid(&record.uid)?;
    if record.id.trim().is_empty() {
        return Err(ReviewError::ReviewHumanId {
            path: path.to_path_buf(),
            uid: record.uid.clone(),
        });
    }
    validate_native_uid(
        &record.requirement_uid,
        REQUIREMENT_UID_PREFIX,
        |uid, expected| ReviewError::NativeUidPrefix { uid, expected },
        |uid| ReviewError::NativeUidUuidV4 { uid },
    )?;
    if record.content_schema != SUPPORTED_REVIEW_CONTENT_SCHEMA {
        return Err(ReviewError::ReviewContentSchema {
            path: path.to_path_buf(),
            uid: record.uid.clone(),
            id: record.id.clone(),
            found: record.content_schema,
            supported: SUPPORTED_REVIEW_CONTENT_SCHEMA,
        });
    }
    if record.reviewer.trim().is_empty() {
        return Err(ReviewError::ReviewReviewer {
            path: path.to_path_buf(),
            uid: record.uid.clone(),
            id: record.id.clone(),
        });
    }
    if chrono::DateTime::parse_from_rfc3339(&record.reviewed_at).is_err() {
        return Err(ReviewError::ReviewTimestamp {
            path: path.to_path_buf(),
            uid: record.uid.clone(),
            id: record.id.clone(),
            value: record.reviewed_at.clone(),
        });
    }
    if record.decision == ReviewDecision::Reject
        && record
            .rationale
            .as_deref()
            .is_none_or(|rationale| rationale.trim().is_empty())
    {
        return Err(ReviewError::ReviewRationale {
            path: path.to_path_buf(),
            uid: record.uid.clone(),
            id: record.id.clone(),
        });
    }
    if let Some(target) = &record.supersedes_review_uid {
        validate_review_uid(target)?;
    }
    Ok(())
}

/// A review uid is `rev_` followed by an RFC 9562 UUIDv4 — the
/// corpus-native uid contract with the review kind's prefix
/// (LLR-114).
fn validate_review_uid(uid: &str) -> Result<(), ReviewError> {
    validate_native_uid(
        uid,
        REVIEW_UID_PREFIX,
        |uid, expected| ReviewError::NativeUidPrefix { uid, expected },
        |uid| ReviewError::NativeUidUuidV4 { uid },
    )
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
#[path = "review_records/insert_error_tests.rs"]
mod insert_error_tests;

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
#[path = "review_records/tests.rs"]
mod tests;
