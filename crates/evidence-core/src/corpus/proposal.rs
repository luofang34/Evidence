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
//! The returned [`AppendOutcome::content_digest`] is a
//! [`ProposalFileDigest`]: SHA-256 over the raw proposal FILE
//! BYTES as written. That domain is distinct from
//! [`ReviewContentDigest`](super::ReviewContentDigest) — SHA-256
//! over the canonical review-content encoding — and the two are
//! never interchangeable.
//!
//! # Semantic content validation
//!
//! [`ProposedRequirementContent`] is validated semantically —
//! non-blank title, valid duplicate-free `derives_from` targets,
//! verification methods sorted and duplicate-free as written — in
//! BOTH append entry points (before anything is written) and on
//! strict read-back, each rule failing closed with its own typed
//! [`ProposalError`] variant.
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
//! [`ProposalError`]: a genuinely absent target is
//! [`ProposalError::ProposalTargetMissing`], a failed evaluation
//! (malformed graph) is [`ProposalError::ProposalLifecycleEvaluation`]
//! carrying the original [`LifecycleError`](super::LifecycleError)
//! as its typed source, and guard rejections name the uid and the
//! offending state or digests.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use uuid::Uuid;

use super::digest::ReviewContentDigest;
use super::graph::CorpusGraph;
use super::lifecycle::{RequirementLifecycle, evaluate_lifecycle};
use super::records::{REQUIREMENT_UID_PREFIX, validate_native_uid};

mod error;
mod record;

pub use error::ProposalError;
pub use record::{
    ProposalAction, ProposalFile, ProposalFileDigest, ProposalRecord, ProposedRequirementContent,
};

/// Highest proposal-file schema version this tool loads (LLR-122).
pub const SUPPORTED_PROPOSAL_SCHEMA: u32 = 1;

/// Typed uid prefix for proposal records (LLR-122).
pub const PROPOSAL_UID_PREFIX: &str = "prop_";

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
    /// Typed digest of the exact bytes written to `path`: SHA-256
    /// over the raw proposal file bytes, a domain distinct from
    /// the review-content digest.
    pub content_digest: ProposalFileDigest,
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
    /// Fails closed with [`ProposalError::ProposalRootMissing`]
    /// when `root` does not exist or cannot be resolved,
    /// [`ProposalError::ProposalRootSymlink`] when it is a symlink
    /// (checked before any following), and
    /// [`ProposalError::ProposalRootNotADirectory`] when it is not
    /// a directory.
    pub fn new(root: &Path) -> Result<Self, ProposalError> {
        let metadata =
            std::fs::symlink_metadata(root).map_err(|_| ProposalError::ProposalRootMissing {
                path: root.to_path_buf(),
            })?;
        if metadata.file_type().is_symlink() {
            return Err(ProposalError::ProposalRootSymlink {
                path: root.to_path_buf(),
            });
        }
        if !metadata.is_dir() {
            return Err(ProposalError::ProposalRootNotADirectory {
                path: root.to_path_buf(),
            });
        }
        let canonical = root
            .canonicalize()
            .map_err(|_| ProposalError::ProposalRootMissing {
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
    /// [`ProposalError::ProposalSubmitter`] on an empty submitter;
    /// the semantic content variants
    /// ([`ProposalError::ProposalContentTitle`],
    /// [`ProposalError::ProposalContentDerivesFrom`],
    /// [`ProposalError::ProposalContentVerificationMethodsOrder`],
    /// [`ProposalError::ProposalContentVerificationMethodsDuplicate`],
    /// and the shared native-uid variants on malformed
    /// `derives_from` targets) before anything is written;
    /// serialization, exclusive-creation, and I/O failures from
    /// the shared append path.
    pub fn append_create_candidate_blocking(
        &self,
        submitter: &str,
        content: ProposedRequirementContent,
    ) -> Result<AppendOutcome, ProposalError> {
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
    /// [`ProposalError::ProposalTargetMissing`] when `base_uid`
    /// names no requirement node in the graph (checked directly);
    /// [`ProposalError::ProposalLifecycleEvaluation`] when the
    /// lifecycle evaluation itself fails — a malformed graph —
    /// with the original [`LifecycleError`](super::LifecycleError)
    /// preserved as the typed source, never stringified;
    /// [`ProposalError::ProposalLifecycle`] (naming the uid and the
    /// evaluated state), [`ProposalError::ProposalDigestMismatch`]
    /// (carrying uid, expected, and actual digests), and the append
    /// failures of [`ProposalStore::append_create_candidate_blocking`].
    pub fn append_revise_candidate_blocking(
        &self,
        graph: &CorpusGraph,
        base_uid: &str,
        expected_current_digest: ReviewContentDigest,
        submitter: &str,
        content: ProposedRequirementContent,
    ) -> Result<AppendOutcome, ProposalError> {
        validate_native_uid(
            base_uid,
            REQUIREMENT_UID_PREFIX,
            |uid, expected| ProposalError::NativeUidPrefix { uid, expected },
            |uid| ProposalError::NativeUidUuidV4 { uid },
        )?;
        if graph.review_content(base_uid).is_none() {
            return Err(ProposalError::ProposalTargetMissing {
                uid: base_uid.to_string(),
            });
        }
        // The requirement node exists, so evaluation cannot hit its
        // missing-requirement path; a failure here means the graph
        // itself is malformed, and the typed source travels with
        // the error.
        let evaluation = evaluate_lifecycle(graph, base_uid)
            .map_err(|source| ProposalError::ProposalLifecycleEvaluation(Box::new(source)))?;
        if evaluation.state != RequirementLifecycle::Candidate {
            return Err(ProposalError::ProposalLifecycle {
                uid: base_uid.to_string(),
                state: evaluation.state,
            });
        }
        if evaluation.current_digest != expected_current_digest {
            return Err(ProposalError::ProposalDigestMismatch {
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
    /// (LLR-122): a malformed or partially written proposal fails
    /// closed here.
    ///
    /// # Errors
    ///
    /// Fails closed with [`ProposalError::ProposalRead`] on I/O
    /// failure, [`ProposalError::ProposalParse`] on malformed TOML,
    /// unknown fields, an unknown action tag, or a malformed
    /// digest, [`ProposalError::ProposalSchema`] on a newer schema
    /// version, and the per-field validation variants (native uid
    /// shapes, [`ProposalError::ProposalSubmitter`],
    /// [`ProposalError::ProposalTimestamp`], and the semantic
    /// content variants) naming the path.
    pub fn read_proposal_blocking(path: &Path) -> Result<ProposalFile, ProposalError> {
        let raw = std::fs::read_to_string(path).map_err(|source| ProposalError::ProposalRead {
            path: path.to_path_buf(),
            source,
        })?;
        let file: ProposalFile =
            toml::from_str(&raw).map_err(|source| ProposalError::ProposalParse {
                path: path.to_path_buf(),
                source,
            })?;
        if file.schema_version > SUPPORTED_PROPOSAL_SCHEMA {
            return Err(ProposalError::ProposalSchema {
                path: path.to_path_buf(),
                found: file.schema_version,
                supported: SUPPORTED_PROPOSAL_SCHEMA,
            });
        }
        record::validate_record(path, &file.proposal)?;
        Ok(file)
    }

    /// Mint the proposal uid and timestamp, validate the record
    /// fields that can fail before any write, serialize, and write
    /// the record beneath the root. Shared tail of both appends.
    fn append_blocking(
        &self,
        submitter: &str,
        action: ProposalAction,
    ) -> Result<AppendOutcome, ProposalError> {
        let proposal_uid = mint_uid(PROPOSAL_UID_PREFIX);
        let path = self.root.join(format!("{proposal_uid}.toml"));
        if submitter.trim().is_empty() {
            return Err(ProposalError::ProposalSubmitter {
                path,
                uid: proposal_uid,
            });
        }
        record::validate_content(&path, &proposal_uid, action.content())?;
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
            toml::to_string_pretty(&file).map_err(|source| ProposalError::ProposalSerialize {
                path: path.clone(),
                source,
            })?;
        write_exclusive_blocking(&path, serialized.as_bytes())?;
        Ok(AppendOutcome {
            path,
            proposal_uid,
            candidate_uid,
            content_digest: ProposalFileDigest::from_hasher_output(crate::hash::sha256(
                serialized.as_bytes(),
            )),
        })
    }
}

/// Mint a corpus-native uid: the kind's prefix plus a fresh UUIDv4.
fn mint_uid(prefix: &str) -> String {
    format!("{prefix}{}", Uuid::new_v4())
}

/// Write `bytes` to `path` with exclusive creation: an existing
/// file is never overwritten (LLR-123). On a mid-write I/O error
/// the partial file is removed best-effort so a truncated proposal
/// cannot masquerade as a complete one on a later listing.
fn write_exclusive_blocking(path: &Path, bytes: &[u8]) -> Result<(), ProposalError> {
    let mut file = match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(file) => file,
        Err(source) if source.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(ProposalError::ProposalExists {
                path: path.to_path_buf(),
            });
        }
        Err(source) => {
            return Err(ProposalError::ProposalWrite {
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
        std::fs::remove_file(path).ok();
        return Err(ProposalError::ProposalWrite {
            path: path.to_path_buf(),
            source,
        });
    }
    Ok(())
}

// Tests live in sibling files pulled in via `#[path]`: shared
// fixtures plus one module per TEST entry.
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
#[path = "proposal/tests_content/tests.rs"]
mod tests_content;
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
