//! Typed lowercase SHA-256 digest over the canonical requirement
//! review-content encoding (LLR-112).
//!
//! [`ReviewContentDigest`] is the only digest representation the
//! review-content API returns: a validated value, never an
//! unconstrained `String`. Every construction boundary — explicit
//! [`ReviewContentDigest::from_hex`] and serde deserialization —
//! validates length and character set and fails closed on malformed
//! input.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::error::CorpusError;

/// A validated lowercase hexadecimal SHA-256 digest of the
/// [`canonical_bytes_v1`](super::canonical_bytes_v1) output for a
/// requirement's review content (LLR-112).
///
/// The value is exactly 64 characters drawn from `[0-9a-f]` — the
/// output alphabet of [`crate::hash::sha256`]. Uppercase or
/// mixed-case input, wrong lengths, non-hex characters, and empty
/// input are rejected with [`CorpusError::InvalidDigest`].
///
/// The digest binds content only: no candidate/approved/rejected/
/// stale lifecycle state is stored in or derived from this type.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ReviewContentDigest(String);

impl ReviewContentDigest {
    /// Validate `hex` as exactly 64 lowercase hexadecimal characters
    /// and wrap it.
    ///
    /// # Errors
    ///
    /// Returns [`CorpusError::InvalidDigest`] naming the length and
    /// character-set expectations when `hex` is empty, short,
    /// overlong, uppercase, mixed-case, or contains non-hex
    /// characters.
    pub fn from_hex(hex: &str) -> Result<Self, CorpusError> {
        if !is_valid_digest_hex(hex) {
            return Err(CorpusError::InvalidDigest {
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

impl fmt::Display for ReviewContentDigest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Serialize for ReviewContentDigest {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for ReviewContentDigest {
    /// Deserialize through the validating constructor so a malformed
    /// stored digest fails closed instead of round-tripping.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Self::from_hex(&raw).map_err(serde::de::Error::custom)
    }
}

/// The digest contract: exactly 64 characters, all lowercase
/// hexadecimal.
fn is_valid_digest_hex(hex: &str) -> bool {
    hex.len() == 64
        && hex
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
mod tests {
    use super::*;

    #[test]
    fn from_hex_rejects_malformed_input() {
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
            let err = ReviewContentDigest::from_hex(&input)
                .expect_err("malformed input must be rejected");
            assert!(
                matches!(err, CorpusError::InvalidDigest { .. }),
                "{name} input must fail with InvalidDigest, got: {err:?}"
            );
        }

        let accepted = ReviewContentDigest::from_hex(&valid).expect("64 lowercase hex is valid");
        assert_eq!(accepted.as_str(), valid);
        assert_eq!(accepted.to_string(), valid);
        let digits = "0123456789".repeat(6) + "abcd";
        assert!(
            ReviewContentDigest::from_hex(&digits).is_ok(),
            "an all-digit digest is valid lowercase hex"
        );
    }

    #[test]
    fn digest_round_trips_through_serde() {
        let digest =
            ReviewContentDigest::from_hex(&"0123456789abcdef".repeat(4)).expect("valid digest");
        let json = serde_json::to_string(&digest).expect("serialize");
        let back: ReviewContentDigest =
            serde_json::from_str(&json).expect("deserialize valid digest");
        assert_eq!(digest, back);

        let malformed = serde_json::to_string("0123456789ABCDEF").expect("serialize string");
        assert!(
            serde_json::from_str::<ReviewContentDigest>(&malformed).is_err(),
            "serde must fail closed on malformed stored digests"
        );
    }
}
