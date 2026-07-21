//! Proposal record schema types, the proposal-file digest domain,
//! and record/content validation (LLR-122, LLR-123).
//!
//! The schema types are strict (`deny_unknown_fields`): every
//! capability not named here is unrepresentable rather than merely
//! rejected. [`ProposalFileDigest`] is its own digest domain —
//! SHA-256 over the raw proposal FILE BYTES — never interchangeable
//! with [`ReviewContentDigest`]. Validation runs twice over the
//! same rules: once before any write in the append path and once
//! on strict read-back, so a hand-written or partially written
//! proposal fails closed with the same typed error an append would
//! have produced.

use std::collections::BTreeSet;
use std::fmt;
use std::path::Path;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::super::digest::{ReviewContentDigest, is_valid_digest_hex};
use super::super::graph::RequirementLayer;
use super::super::records::{REQUIREMENT_UID_PREFIX, validate_native_uid};
use super::PROPOSAL_UID_PREFIX;
use super::error::ProposalError;

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

impl ProposalAction {
    /// The proposed content, regardless of action kind.
    pub(super) fn content(&self) -> &ProposedRequirementContent {
        match self {
            ProposalAction::CreateCandidate { content, .. }
            | ProposalAction::ReviseCandidate { content, .. } => content,
        }
    }
}

/// Full replacement content for a candidate (LLR-122).
///
/// Mirrors the [`RequirementReviewContentV1`](super::super::RequirementReviewContentV1)
/// projection fields and nothing else: no uid, human id, owner,
/// sort key, implementation modules, governed surfaces, or emitted
/// diagnostics can be smuggled through a proposal.
///
/// # Semantic contract
///
/// Beyond the serde shape, the content is validated semantically
/// on append and on read-back: `title` must be non-empty after
/// be non-empty after trimming; every `derives_from` entry must be
/// a valid `req_<UUIDv4>` and the list duplicate-free; and
/// `verification_methods` — allowed to be empty, mirroring the
/// review-content contract — must be sorted and duplicate-free as
/// written. The ordering rule is deliberately stricter than the
/// projection's [`canonicalize`](super::super::RequirementReviewContentV1::canonicalize),
/// which sorts silently: a proposal must arrive in canonical form
/// so the bytes a human reviews are already the canonical bytes.
/// `description`, `rationale`, `scope`, `category`, and `source`
/// stay optional free text; `layer` is serde-validated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProposedRequirementContent {
    /// One-line requirement title; non-empty after trimming.
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
    /// Verification methods; sorted and duplicate-free as written.
    #[serde(default)]
    pub verification_methods: Vec<String>,
    /// Canonical `derives_from` target uids; each a valid
    /// `req_<UUIDv4>`, the list duplicate-free.
    #[serde(default)]
    pub derives_from: Vec<String>,
}

/// On-disk shape of a proposal file. Strict: unknown fields are a
/// parse error (LLR-122).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProposalFile {
    /// File schema version; newer than
    /// [`SUPPORTED_PROPOSAL_SCHEMA`](super::SUPPORTED_PROPOSAL_SCHEMA)
    /// refuses to load.
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

/// A validated lowercase hexadecimal SHA-256 digest over the raw
/// proposal FILE BYTES exactly as written to disk (LLR-123).
///
/// This is its own digest domain, distinct from
/// [`ReviewContentDigest`]: a proposal file digest binds the exact
/// serialized TOML bytes of one proposal file, while a
/// review-content digest binds the canonical review-content
/// encoding of a requirement. The two domains must never be
/// interchangeable, and no API accepts one where the other is
/// meant.
///
/// The value is exactly 64 characters drawn from `[0-9a-f]` — the
/// output alphabet of [`crate::hash::sha256`]. Uppercase or
/// mixed-case input, wrong lengths, non-hex characters, and empty
/// input are rejected with [`ProposalError::InvalidFileDigest`].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProposalFileDigest(String);

impl ProposalFileDigest {
    /// Validate `hex` as exactly 64 lowercase hexadecimal characters
    /// and wrap it.
    ///
    /// # Errors
    ///
    /// Returns [`ProposalError::InvalidFileDigest`] naming the
    /// length and character-set expectations when `hex` is empty,
    /// short, overlong, uppercase, mixed-case, or contains non-hex
    /// characters.
    pub fn from_hex(hex: &str) -> Result<Self, ProposalError> {
        if !is_valid_digest_hex(hex) {
            return Err(ProposalError::InvalidFileDigest {
                input: hex.to_string(),
            });
        }
        Ok(Self(hex.to_string()))
    }

    /// The 64-character lowercase hexadecimal digest string.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Wrap SHA-256 hasher output, which satisfies the digest
    /// contract by construction.
    pub(crate) fn from_hasher_output(hex: String) -> Self {
        debug_assert!(
            is_valid_digest_hex(&hex),
            "sha256 hex output must satisfy the digest contract"
        );
        Self(hex)
    }
}

impl fmt::Display for ProposalFileDigest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Serialize for ProposalFileDigest {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for ProposalFileDigest {
    /// Deserialize through the validating constructor so a malformed
    /// stored digest fails closed instead of round-tripping.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Self::from_hex(&raw).map_err(serde::de::Error::custom)
    }
}

/// Validate a record's fields in declaration order; the first
/// failure wins, so error precedence is deterministic (LLR-122).
/// Runs on strict read-back; the append path runs the same checks
/// it can fail (submitter, content) before writing.
pub(super) fn validate_record(path: &Path, record: &ProposalRecord) -> Result<(), ProposalError> {
    validate_native_uid(
        &record.uid,
        PROPOSAL_UID_PREFIX,
        |uid, expected| ProposalError::NativeUidPrefix { uid, expected },
        |uid| ProposalError::NativeUidUuidV4 { uid },
    )?;
    if record.submitter.trim().is_empty() {
        return Err(ProposalError::ProposalSubmitter {
            path: path.to_path_buf(),
            uid: record.uid.clone(),
        });
    }
    if chrono::DateTime::parse_from_rfc3339(&record.submitted_at).is_err() {
        return Err(ProposalError::ProposalTimestamp {
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
                |uid, expected| ProposalError::NativeUidPrefix { uid, expected },
                |uid| ProposalError::NativeUidUuidV4 { uid },
            )?;
        }
        ProposalAction::ReviseCandidate { base_uid, .. } => {
            validate_native_uid(
                base_uid,
                REQUIREMENT_UID_PREFIX,
                |uid, expected| ProposalError::NativeUidPrefix { uid, expected },
                |uid| ProposalError::NativeUidUuidV4 { uid },
            )?;
        }
    }
    validate_content(path, &record.uid, record.action.content())
}

/// Validate the semantic content contract (LLR-122): non-blank
/// `title`; `derives_from` entries each a valid `req_<UUIDv4>` with
/// no duplicates; `verification_methods` sorted and duplicate-free
/// as written. The first failure in document order wins, so error
/// precedence is deterministic.
pub(super) fn validate_content(
    path: &Path,
    uid: &str,
    content: &ProposedRequirementContent,
) -> Result<(), ProposalError> {
    if content.title.trim().is_empty() {
        return Err(ProposalError::ProposalContentTitle {
            path: path.to_path_buf(),
            uid: uid.to_string(),
        });
    }
    let mut seen = BTreeSet::new();
    for target in &content.derives_from {
        validate_native_uid(
            target,
            REQUIREMENT_UID_PREFIX,
            |uid, expected| ProposalError::NativeUidPrefix { uid, expected },
            |uid| ProposalError::NativeUidUuidV4 { uid },
        )?;
        if !seen.insert(target) {
            return Err(ProposalError::ProposalContentDerivesFrom {
                path: path.to_path_buf(),
                uid: uid.to_string(),
                target: target.clone(),
            });
        }
    }
    for pair in content.verification_methods.windows(2) {
        let (first, second) = (&pair[0], &pair[1]);
        if first == second {
            return Err(ProposalError::ProposalContentVerificationMethodsDuplicate {
                path: path.to_path_buf(),
                uid: uid.to_string(),
                method: first.clone(),
            });
        }
        if first > second {
            return Err(ProposalError::ProposalContentVerificationMethodsOrder {
                path: path.to_path_buf(),
                uid: uid.to_string(),
                first: first.clone(),
                second: second.clone(),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test setup failures should panic immediately"
)]
mod tests {
    use super::*;

    #[test]
    fn file_digest_from_hex_rejects_malformed_input() {
        let valid = "a".repeat(64);
        let cases: [(&str, String); 6] = [
            ("empty", String::new()),
            ("short", valid[..63].to_string()),
            ("overlong", format!("{valid}0")),
            ("uppercase", valid.to_uppercase()),
            ("mixed case", format!("{}A", &valid[..63])),
            ("non-hex", format!("{}g", &valid[..63])),
        ];
        for (name, input) in cases {
            let err =
                ProposalFileDigest::from_hex(&input).expect_err("malformed input must be rejected");
            assert!(
                matches!(err, ProposalError::InvalidFileDigest { .. }),
                "{name} input must fail with InvalidFileDigest, got: {err:?}"
            );
        }

        let accepted = ProposalFileDigest::from_hex(&valid).expect("64 lowercase hex is valid");
        assert_eq!(accepted.as_str(), valid);
        assert_eq!(accepted.to_string(), valid);
    }

    #[test]
    fn file_digest_round_trips_through_serde() {
        let digest =
            ProposalFileDigest::from_hex(&"0123456789abcdef".repeat(4)).expect("valid digest");
        let json = serde_json::to_string(&digest).expect("serialize");
        let back: ProposalFileDigest =
            serde_json::from_str(&json).expect("deserialize valid digest");
        assert_eq!(digest, back);

        let malformed = serde_json::to_string("0123456789ABCDEF").expect("serialize string");
        assert!(
            serde_json::from_str::<ProposalFileDigest>(&malformed).is_err(),
            "serde must fail closed on malformed stored digests"
        );
    }
}
