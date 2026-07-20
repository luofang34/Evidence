//! Append-only candidate-proposal store — the one agent-facing
//! write capability over the corpus (LLR-122, LLR-123, LLR-124).
//!
//! # Authority boundary
//!
//! Agent-facing code receives [`ProposalStore`] and nothing else:
//! review creation, lifecycle mutation, source mutation, and
//! baseline writes are absent from this interface, and
//! [`ProposalAction`] makes approval, rejection, deletion, review
//! mutation, source mutation, baseline replacement, and direct
//! file-path writes unrepresentable. Authority is enforced through
//! types and available operations — there is deliberately no
//! `actor` or `is_human` field anywhere in this module.
//! `submitter` is audit identity, never authority. `submitted_at`
//! is metadata, never ordering authority.
//!
//! # Append, never apply
//!
//! Appending a proposal never applies it. Human acceptance and
//! public queue/review surfaces are separate work above this
//! layer; the corpus baseline, review records, and frozen sources
//! remain byte-for-byte unchanged by any submission. The store
//! writes only a generated basename beneath a caller-supplied
//! proposal root and never touches anything outside it.
//!
//! # Store-minted identities and fail-closed writes
//!
//! The store mints every identity itself: the `prop_<UUIDv4>`
//! proposal uid, the `req_<UUIDv4>` candidate uid for a
//! [`ProposalAction::CreateCandidate`], and the RFC 3339
//! `submitted_at` timestamp — no caller-supplied uid, clock, or
//! path component ever reaches the record or its filename. Writes
//! use exclusive creation ([`OpenOptions::create_new`]), so an
//! existing proposal is never overwritten and concurrent
//! submissions cannot collide; a mid-write I/O error removes the
//! partial file on a best-effort basis. A malformed or partially
//! written proposal fails closed when read through
//! [`ProposalStore::read_proposal_blocking`].
//!
//! # Revision guards
//!
//! [`ProposalStore::append_revise_candidate_blocking`] verifies,
//! before writing anything, that the base uid is a valid
//! `req_<UUIDv4>` naming an existing requirement whose evaluated
//! lifecycle is [`RequirementLifecycle::Candidate`] — an approved,
//! rejected, or stale target is rejected, so a proposal can never
//! demote approved content — and that the caller's expected digest
//! equals the requirement's current review-content digest
//! (optimistic concurrency). Every failure is a typed
//! [`CorpusError`] carrying the uid and the offending state or
//! digests.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::digest::ReviewContentDigest;
use super::error::CorpusError;
use super::graph::{CorpusGraph, RequirementLayer};
use super::lifecycle::{RequirementLifecycle, evaluate_lifecycle};
use super::records::{REQUIREMENT_UID_PREFIX, validate_native_uid};

/// Highest proposal-file schema version this tool loads (LLR-122).
pub const SUPPORTED_PROPOSAL_SCHEMA: u32 = 1;

/// Typed uid prefix for proposal records (LLR-122).
pub const PROPOSAL_UID_PREFIX: &str = "prop_";

/// The only two representable proposal actions (LLR-122).
///
/// Serde snake_case with the tag named `action`; any other tag —
/// `approve`, `reject`, `delete`, a review/source/baseline/file
/// mutation — fails deserialization, so excluded capabilities are
/// unrepresentable rather than merely rejected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProposalAction {
    /// Propose a complete new candidate requirement. The store
    /// mints `candidate_uid` (`req_<UUIDv4>`); the proposed content
    /// carries no identity of its own.
    CreateCandidate {
        /// Store-minted uid of the candidate this proposal would
        /// create: `req_<UUIDv4>`, validated on read.
        candidate_uid: String,
        /// Complete content of the proposed candidate.
        content: ProposedRequirementContent,
    },
    /// Propose complete replacement content for an existing
    /// candidate, carrying the expected current content digest for
    /// optimistic concurrency.
    ReviseCandidate {
        /// Uid of the candidate to revise: `req_<UUIDv4>`,
        /// validated on read.
        base_uid: String,
        /// Digest the submitter believes the current content has;
        /// the append fails closed when it has moved.
        expected_current_digest: ReviewContentDigest,
        /// Complete replacement content.
        content: ProposedRequirementContent,
    },
}

/// Full replacement content for a candidate (LLR-122).
///
/// Mirrors the [`RequirementReviewContentV1`](super::RequirementReviewContentV1)
/// projection fields and nothing else: no uid, human id, owner,
/// sort key, implementation modules, governed surfaces, or emitted
/// diagnostics can be smuggled through a proposal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProposedRequirementContent {
    /// One-line requirement title.
    pub title: String,
    /// Decomposition layer.
    pub layer: RequirementLayer,
    /// Normative requirement description.
    #[serde(default)]
    pub description: Option<String>,
    /// Normative rationale.
    #[serde(default)]
    pub rationale: Option<String>,
    /// Requirement scope.
    #[serde(default)]
    pub scope: Option<String>,
    /// Requirement category.
    #[serde(default)]
    pub category: Option<String>,
    /// Source reference.
    #[serde(default)]
    pub source: Option<String>,
    /// Verification methods.
    #[serde(default)]
    pub verification_methods: Vec<String>,
    /// Canonical `derives_from` target uids.
    #[serde(default)]
    pub derives_from: Vec<String>,
}

/// On-disk shape of a proposal file. Strict: unknown fields are a
/// parse error (LLR-122).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProposalFile {
    /// File schema version; newer than
    /// [`SUPPORTED_PROPOSAL_SCHEMA`] refuses to load.
    pub schema_version: u32,
    /// The proposal record.
    pub proposal: ProposalRecord,
}

/// One proposal record (LLR-122).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProposalRecord {
    /// Permanent identity: `prop_<UUIDv4>`, store-minted and
    /// validated on read.
    pub uid: String,
    /// Submitter audit identity; non-empty after trimming. Audit
    /// metadata only — never accepted as proof of authority.
    pub submitter: String,
    /// RFC 3339 submission timestamp, store-minted. Metadata only;
    /// never ordering authority.
    pub submitted_at: String,
    /// The proposed action — exactly the two representable kinds.
    pub action: ProposalAction,
}

/// The result of one successful append (LLR-123).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppendOutcome {
    /// Path of the created proposal file, beneath the store's
    /// canonical root.
    pub path: PathBuf,
    /// Store-minted `prop_<UUIDv4>` proposal uid; the file basename
    /// is `<proposal_uid>.toml`.
    pub proposal_uid: String,
    /// Store-minted `req_<UUIDv4>` of the proposed candidate for a
    /// create action; `None` for a revision.
    pub candidate_uid: Option<String>,
    /// Typed digest of the exact bytes written to `path`.
    pub content_digest: ReviewContentDigest,
}

/// The append-only proposal store (LLR-123).
///
/// Construction validates the root: it must exist, be a directory,
/// and not be a symlink; the stored path is canonicalized. The
/// store exposes only append and strict read-back — there is no
/// update, delete, apply, review, or baseline operation.
#[derive(Debug, Clone)]
pub struct ProposalStore {
    root: PathBuf,
}

impl ProposalStore {
    /// Open the proposal root at `root`.
    ///
    /// # Errors
    ///
    /// Fails closed with [`CorpusError::ProposalRootMissing`] when
    /// `root` does not exist or cannot be resolved,
    /// [`CorpusError::ProposalRootSymlink`] when it is a symlink
    /// (checked before any following), and
    /// [`CorpusError::ProposalRootNotADirectory`] when it is not a
    /// directory.
    pub fn new(root: &Path) -> Result<Self, CorpusError> {
        let metadata =
            std::fs::symlink_metadata(root).map_err(|_| CorpusError::ProposalRootMissing {
                path: root.to_path_buf(),
            })?;
        if metadata.file_type().is_symlink() {
            return Err(CorpusError::ProposalRootSymlink {
                path: root.to_path_buf(),
            });
        }
        if !metadata.is_dir() {
            return Err(CorpusError::ProposalRootNotADirectory {
                path: root.to_path_buf(),
            });
        }
        let canonical = root
            .canonicalize()
            .map_err(|_| CorpusError::ProposalRootMissing {
                path: root.to_path_buf(),
            })?;
        Ok(Self { root: canonical })
    }

    /// Append a proposal for a complete new candidate requirement
    /// (LLR-123). The store mints both the proposal uid and the
    /// candidate uid — a caller cannot squat or collide identities.
    ///
    /// # Errors
    ///
    /// [`CorpusError::ProposalSubmitter`] on an empty submitter;
    /// serialization, exclusive-creation, and I/O failures from the
    /// shared append path.
    pub fn append_create_candidate_blocking(
        &self,
        submitter: &str,
        content: ProposedRequirementContent,
    ) -> Result<AppendOutcome, CorpusError> {
        let candidate_uid = mint_uid(REQUIREMENT_UID_PREFIX);
        self.append_blocking(
            submitter,
            ProposalAction::CreateCandidate {
                candidate_uid,
                content,
            },
        )
    }

    /// Append a proposal replacing the full content of an existing
    /// candidate (LLR-124). Every guard runs before any write:
    /// `base_uid` must be a valid `req_<UUIDv4>` naming an existing
    /// requirement in `graph` whose evaluated lifecycle is
    /// [`RequirementLifecycle::Candidate`], and
    /// `expected_current_digest` must equal its current
    /// review-content digest.
    ///
    /// # Errors
    ///
    /// The shared native-uid variants on a malformed `base_uid`;
    /// [`CorpusError::ProposalTargetMissing`],
    /// [`CorpusError::ProposalLifecycle`] (naming the uid and the
    /// evaluated state), [`CorpusError::ProposalDigestMismatch`]
    /// (carrying uid, expected, and actual digests), and the append
    /// failures of [`ProposalStore::append_create_candidate_blocking`].
    pub fn append_revise_candidate_blocking(
        &self,
        graph: &CorpusGraph,
        base_uid: &str,
        expected_current_digest: ReviewContentDigest,
        submitter: &str,
        content: ProposedRequirementContent,
    ) -> Result<AppendOutcome, CorpusError> {
        validate_native_uid(
            base_uid,
            REQUIREMENT_UID_PREFIX,
            |uid, expected| CorpusError::NativeUidPrefix { uid, expected },
            |uid| CorpusError::NativeUidUuidV4 { uid },
        )?;
        if graph.review_content(base_uid).is_none() {
            return Err(CorpusError::ProposalTargetMissing {
                uid: base_uid.to_string(),
            });
        }
        // The requirement node exists, so evaluation cannot hit its
        // missing-requirement path; the error arm is mapped
        // defensively to keep the match honest.
        let evaluation = evaluate_lifecycle(graph, base_uid).map_err(|_| {
            CorpusError::ProposalTargetMissing {
                uid: base_uid.to_string(),
            }
        })?;
        if evaluation.state != RequirementLifecycle::Candidate {
            return Err(CorpusError::ProposalLifecycle {
                uid: base_uid.to_string(),
                state: evaluation.state,
            });
        }
        if evaluation.current_digest != expected_current_digest {
            return Err(CorpusError::ProposalDigestMismatch {
                uid: base_uid.to_string(),
                expected: expected_current_digest,
                actual: evaluation.current_digest,
            });
        }
        self.append_blocking(
            submitter,
            ProposalAction::ReviseCandidate {
                base_uid: base_uid.to_string(),
                expected_current_digest,
                content,
            },
        )
    }

    /// Read and strictly validate the proposal file at `path`
    /// (LLR-122). This is the future apply-path seam: a malformed
    /// or partially written proposal fails closed here.
    ///
    /// # Errors
    ///
    /// Fails closed with [`CorpusError::ProposalRead`] on I/O
    /// failure, [`CorpusError::ProposalParse`] on malformed TOML,
    /// unknown fields, an unknown action tag, or a malformed
    /// digest, [`CorpusError::ProposalSchema`] on a newer schema
    /// version, and the per-field validation variants (native uid
    /// shapes, [`CorpusError::ProposalSubmitter`],
    /// [`CorpusError::ProposalTimestamp`]) naming the path.
    pub fn read_proposal_blocking(path: &Path) -> Result<ProposalFile, CorpusError> {
        let raw = std::fs::read_to_string(path).map_err(|source| CorpusError::ProposalRead {
            path: path.to_path_buf(),
            source,
        })?;
        let file: ProposalFile =
            toml::from_str(&raw).map_err(|source| CorpusError::ProposalParse {
                path: path.to_path_buf(),
                source,
            })?;
        if file.schema_version > SUPPORTED_PROPOSAL_SCHEMA {
            return Err(CorpusError::ProposalSchema {
                path: path.to_path_buf(),
                found: file.schema_version,
                supported: SUPPORTED_PROPOSAL_SCHEMA,
            });
        }
        validate_record(path, &file.proposal)?;
        Ok(file)
    }

    /// Mint the proposal uid and timestamp, serialize, and write
    /// the record beneath the root. Shared tail of both appends.
    fn append_blocking(
        &self,
        submitter: &str,
        action: ProposalAction,
    ) -> Result<AppendOutcome, CorpusError> {
        let proposal_uid = mint_uid(PROPOSAL_UID_PREFIX);
        let path = self.root.join(format!("{proposal_uid}.toml"));
        if submitter.trim().is_empty() {
            return Err(CorpusError::ProposalSubmitter {
                path,
                uid: proposal_uid,
            });
        }
        let candidate_uid = match &action {
            ProposalAction::CreateCandidate { candidate_uid, .. } => Some(candidate_uid.clone()),
            ProposalAction::ReviseCandidate { .. } => None,
        };
        let file = ProposalFile {
            schema_version: SUPPORTED_PROPOSAL_SCHEMA,
            proposal: ProposalRecord {
                uid: proposal_uid.clone(),
                submitter: submitter.to_string(),
                submitted_at: chrono::Utc::now().to_rfc3339(),
                action,
            },
        };
        let serialized =
            toml::to_string_pretty(&file).map_err(|source| CorpusError::ProposalSerialize {
                path: path.clone(),
                source,
            })?;
        write_exclusive_blocking(&path, serialized.as_bytes())?;
        Ok(AppendOutcome {
            path,
            proposal_uid,
            candidate_uid,
            content_digest: ReviewContentDigest::from_hasher_output(crate::hash::sha256(
                serialized.as_bytes(),
            )),
        })
    }
}

/// Mint a corpus-native uid: the kind's prefix plus a fresh UUIDv4.
fn mint_uid(prefix: &str) -> String {
    format!("{prefix}{}", Uuid::new_v4())
}

/// Validate a record's fields in declaration order; the first
/// failure wins, so error precedence is deterministic (LLR-122).
fn validate_record(path: &Path, record: &ProposalRecord) -> Result<(), CorpusError> {
    validate_native_uid(
        &record.uid,
        PROPOSAL_UID_PREFIX,
        |uid, expected| CorpusError::NativeUidPrefix { uid, expected },
        |uid| CorpusError::NativeUidUuidV4 { uid },
    )?;
    if record.submitter.trim().is_empty() {
        return Err(CorpusError::ProposalSubmitter {
            path: path.to_path_buf(),
            uid: record.uid.clone(),
        });
    }
    if chrono::DateTime::parse_from_rfc3339(&record.submitted_at).is_err() {
        return Err(CorpusError::ProposalTimestamp {
            path: path.to_path_buf(),
            uid: record.uid.clone(),
            value: record.submitted_at.clone(),
        });
    }
    match &record.action {
        ProposalAction::CreateCandidate { candidate_uid, .. } => {
            validate_native_uid(
                candidate_uid,
                REQUIREMENT_UID_PREFIX,
                |uid, expected| CorpusError::NativeUidPrefix { uid, expected },
                |uid| CorpusError::NativeUidUuidV4 { uid },
            )?;
        }
        ProposalAction::ReviseCandidate { base_uid, .. } => {
            validate_native_uid(
            base_uid,
            REQUIREMENT_UID_PREFIX,
            |uid, expected| CorpusError::NativeUidPrefix { uid, expected },
            |uid| CorpusError::NativeUidUuidV4 { uid },
        )?;
        }
    }
    Ok(())
}

/// Write `bytes` to `path` with exclusive creation: an existing
/// file is never overwritten (LLR-123). On a mid-write I/O error
/// the partial file is removed best-effort so a truncated proposal
/// cannot masquerade as a complete one on a later listing.
fn write_exclusive_blocking(path: &Path, bytes: &[u8]) -> Result<(), CorpusError> {
    let mut file = match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(file) => file,
        Err(source) if source.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(CorpusError::ProposalExists {
                path: path.to_path_buf(),
            });
        }
        Err(source) => {
            return Err(CorpusError::ProposalWrite {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    if let Err(source) = file.write_all(bytes) {
        drop(file);
        // Best-effort: the removal itself may fail (e.g. the write
        // failed because the filesystem went away); the typed write
        // error is the authoritative signal either way.
        drop(std::fs::remove_file(path));
        return Err(CorpusError::ProposalWrite {
            path: path.to_path_buf(),
            source,
        });
    }
    Ok(())
}

// Tests live in sibling files pulled in via `#[path]` so this
// facade stays under the 500-line workspace limit: shared fixtures
// plus one module per TEST entry.
#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
#[path = "proposal/tests.rs"]
mod tests;
#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
#[path = "proposal/tests_guards/tests.rs"]
mod tests_guards;
#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
#[path = "proposal/tests_support.rs"]
mod tests_support;
