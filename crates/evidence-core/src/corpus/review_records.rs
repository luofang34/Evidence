//! Corpus-native human review decision records (LLR-114, LLR-170).
//!
//! A review file is a strict, `schema_version`-gated TOML document
//! named by the corpus index's `reviews` kind. Each record binds one
//! reviewer's decision to a typed target — the closed
//! [`ReviewTarget`] set of a requirement or a curated patch — and
//! the exact reviewed-content digest of the target kind's canonical
//! projection (LLR-111, LLR-167). Everything degenerate fails
//! closed with a typed [`ReviewError`] naming the file path and the
//! record's id and uid: unknown fields, a newer file or content
//! schema, a generic target kind (the kind enum is closed —
//! unrepresentable), a cross-kind uid, a mixed or missing target
//! shape, malformed uids, digests, or timestamps, an empty reviewer
//! identity or human id, and a rejection without a rationale.
//!
//! # Schema transition (LLR-170)
//!
//! - **Schema 1** records carry the legacy `requirement_uid` field
//!   and load as [`ReviewTarget::Requirement`] with identical
//!   semantics, digests, and error precedence as before the
//!   transition.
//! - **Schema 2** records carry `target = { kind, uid }` with a
//!   closed snake_case kind enum; the uid must satisfy the kind's
//!   typed prefix (`req_<UUIDv4>` or `patch_<UUIDv4>`). The kind
//!   owns the reviewed-content projection the digest covers.
//! - The two shapes never mix: a typed target in a schema-1
//!   record, a legacy field in a schema-2 record, both fields, or
//!   neither fails closed with a typed shape error.
//! - Older tools fail closed on schema-2 files through the existing
//!   schema-version gate — target-kind fields are never silently
//!   ignored. Mixed legacy/new files load deterministically: each
//!   file's declared schema drives its own record shape.
//!
//! `reviewer` and `reviewed_at` are audit metadata: the reviewer
//! identity is recorded, never accepted as proof that a caller is
//! human, and the timestamp never chooses an effective decision.
//! Review file layout and load order are non-semantic.

use std::path::Path;

use serde::Deserialize;

use super::digest::ReviewContentDigest;
use super::graph::{
    CorpusGraph, EdgeKind, Node, ReviewDecision, ReviewNode, ReviewTarget, ReviewTargetKind,
};
use super::records::{REQUIREMENT_UID_PREFIX, validate_native_uid};
use super::source_patch::records::PATCH_UID_PREFIX;
use error::ReviewError;

pub(super) mod error;

/// Highest review-file schema version this tool loads.
pub const SUPPORTED_REVIEW_SCHEMA: u32 = 2;

/// The review-content projection version a record's digest must
/// cover (LLR-111, LLR-167). Each target kind owns its v1
/// projection; both are content schema 1.
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

/// The schema-2 typed target table (LLR-170). Strict: unknown
/// fields are a parse error, and the closed [`ReviewTargetKind`]
/// enum makes a generic kind string unrepresentable.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewTargetWire {
    /// The closed target kind.
    pub kind: ReviewTargetKind,
    /// The target artifact's uid; validated against the kind's
    /// typed prefix at record load.
    pub uid: String,
}

/// One human review decision record. Strict: unknown fields are a
/// parse error. `requirement_uid` and `target` are both optional
/// at deserialization so the file's declared `schema_version` —
/// not the TOML shape alone — decides which one is required; the
/// shape checks in [`validate_record`] fail closed on a mixed,
/// missing, or misplaced target.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewRecord {
    /// Permanent identity: `rev_<UUIDv4>`.
    pub uid: String,
    /// Human-readable identifier (e.g. `REV-001`); non-empty.
    pub id: String,
    /// Legacy schema-1 target: uid of the reviewed requirement,
    /// `req_<UUIDv4>`. Forbidden in schema-2 records.
    #[serde(default)]
    pub requirement_uid: Option<String>,
    /// Schema-2 typed target. Forbidden in schema-1 records.
    #[serde(default)]
    pub target: Option<ReviewTargetWire>,
    /// Review-content projection version the digest covers; must
    /// equal [`SUPPORTED_REVIEW_CONTENT_SCHEMA`].
    pub content_schema: u32,
    /// Exact digest of the reviewed canonical content; serde
    /// deserialization validates length and character set. The
    /// target kind owns the projection the digest covers.
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
        let target = validate_record(path, file.schema_version, &record)?;
        let mut edges = vec![(EdgeKind::Reviews, target.uid().to_string())];
        if let Some(target_uid) = &record.supersedes_review_uid {
            edges.push((EdgeKind::Supersedes, target_uid.clone()));
        }
        graph
            .insert(Node::Review(ReviewNode {
                uid: record.uid,
                id: record.id,
                target,
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

/// Resolve the record's typed target from the file's declared
/// schema (LLR-170): schema 1 requires the legacy
/// `requirement_uid` field and forbids `target`; schema 2 requires
/// the typed `target` table and forbids `requirement_uid`. Every
/// other combination fails closed with a typed shape error.
fn resolve_target(
    path: &Path,
    schema_version: u32,
    record: &ReviewRecord,
) -> Result<ReviewTarget, ReviewError> {
    let shape_err = |expected: &'static str, found: &'static str| ReviewError::ReviewTargetShape {
        path: path.to_path_buf(),
        uid: record.uid.clone(),
        id: record.id.clone(),
        schema_version,
        expected,
        found,
    };
    // Only schema versions above the supported maximum are rejected
    // before this point; anything below 1 reads as the legacy
    // layout, as before the transition.
    let legacy = schema_version < 2;
    match (legacy, &record.requirement_uid, &record.target) {
        (true, Some(requirement_uid), None) => {
            validate_native_uid(
                requirement_uid,
                REQUIREMENT_UID_PREFIX,
                |uid, expected| ReviewError::NativeUidPrefix { uid, expected },
                |uid| ReviewError::NativeUidUuidV4 { uid },
            )?;
            Ok(ReviewTarget::Requirement(requirement_uid.clone()))
        }
        (true, _, _) => Err(shape_err(
            "the legacy `requirement_uid` field and no `target` table",
            "a missing `requirement_uid` or a present `target` table",
        )),
        (false, None, Some(target)) => {
            let prefix = match target.kind {
                ReviewTargetKind::Requirement => REQUIREMENT_UID_PREFIX,
                ReviewTargetKind::CuratedPatch => PATCH_UID_PREFIX,
            };
            validate_native_uid(
                &target.uid,
                prefix,
                |uid, expected| ReviewError::NativeUidPrefix { uid, expected },
                |uid| ReviewError::NativeUidUuidV4 { uid },
            )?;
            Ok(match target.kind {
                ReviewTargetKind::Requirement => ReviewTarget::Requirement(target.uid.clone()),
                ReviewTargetKind::CuratedPatch => ReviewTarget::CuratedPatch(target.uid.clone()),
            })
        }
        (false, _, _) => Err(shape_err(
            "the typed `target` table and no `requirement_uid` field",
            "a missing `target` table or a present `requirement_uid` field",
        )),
    }
}

/// Validate one record's fields in declaration order; the first
/// failure wins, so error precedence is deterministic (LLR-114).
/// The target shape check sits where the legacy `requirement_uid`
/// check always ran — after the human id, before the content
/// schema — so schema-1 records fail in the identical order as
/// before the transition (LLR-170). Returns the resolved typed
/// target.
fn validate_record(
    path: &Path,
    schema_version: u32,
    record: &ReviewRecord,
) -> Result<ReviewTarget, ReviewError> {
    validate_review_uid(&record.uid)?;
    if record.id.trim().is_empty() {
        return Err(ReviewError::ReviewHumanId {
            path: path.to_path_buf(),
            uid: record.uid.clone(),
        });
    }
    let target = resolve_target(path, schema_version, record)?;
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
    if let Some(predecessor) = &record.supersedes_review_uid {
        validate_review_uid(predecessor)?;
    }
    Ok(target)
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

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
#[path = "review_records/typed_target_tests.rs"]
mod typed_target_tests;
