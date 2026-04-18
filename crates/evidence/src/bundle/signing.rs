//! HMAC-SHA256 signing of a bundle's `SHA256SUMS` + `index.json` envelope.

use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::diagnostic::{DiagnosticCode, Location, Severity};

type HmacSha256 = Hmac<Sha256>;

/// Errors returned by [`sign_bundle`] / [`verify_bundle_signature`].
#[derive(Debug, Error)]
pub enum SigningError {
    /// Failed to read one of the envelope inputs or the signature file.
    #[error("reading {path}")]
    Read {
        /// Bundle-relative filename (`SHA256SUMS`, `index.json`, `BUNDLE.sig`).
        path: String,
        /// Underlying OS error.
        #[source]
        source: std::io::Error,
    },
    /// Failed to write `BUNDLE.sig`.
    #[error("writing {path}")]
    Write {
        /// Bundle-relative filename being written (always `BUNDLE.sig`).
        path: String,
        /// Underlying OS error.
        #[source]
        source: std::io::Error,
    },
    /// The provided HMAC key had an invalid length for SHA-256.
    #[error("invalid HMAC key: {reason}")]
    InvalidKey {
        /// Human-readable reason from the `hmac` crate.
        reason: String,
    },
    /// `BUNDLE.sig` contained non-hex bytes.
    #[error("BUNDLE.sig contains invalid hex")]
    InvalidSignatureHex(#[source] hex::FromHexError),
}

impl DiagnosticCode for SigningError {
    fn code(&self) -> &'static str {
        match self {
            SigningError::Read { .. } => "SIGN_READ_FAILED",
            SigningError::Write { .. } => "SIGN_WRITE_FAILED",
            SigningError::InvalidKey { .. } => "SIGN_INVALID_KEY",
            SigningError::InvalidSignatureHex(_) => "SIGN_INVALID_SIGNATURE_HEX",
        }
    }

    fn severity(&self) -> Severity {
        Severity::Error
    }

    fn location(&self) -> Option<Location> {
        let file = match self {
            SigningError::Read { path, .. } | SigningError::Write { path, .. } => {
                Some(PathBuf::from(path))
            }
            SigningError::InvalidKey { .. } | SigningError::InvalidSignatureHex(_) => None,
        };
        file.map(|file| Location {
            file: Some(file),
            ..Location::default()
        })
    }
}

/// HMAC envelope layout: length-prefixed concatenation of
/// `SHA256SUMS` and `index.json` as they live on disk, no
/// canonicalization at verify time. The length prefixes frame the
/// two inputs unambiguously so no pair of (A, B) byte strings can
/// collide onto the same MAC input as another (A', B').
///
/// ```text
///   u64_be(|SHA256SUMS|) || SHA256SUMS || u64_be(|index.json|) || index.json
/// ```
///
/// `SHA256SUMS` already covers the content layer (env.json,
/// inputs/outputs hashes, deterministic-manifest.json, trace outputs,
/// captured stdout/stderr). Extending the envelope to include
/// `index.json`'s on-disk bytes closes the metadata-layer tampering
/// gap: editing any index field (engine_git_sha, dal_map,
/// test_summary, trace_outputs, schema versions, timestamp…) without
/// the HMAC key rotates the MAC.
///
/// We feed disk bytes verbatim rather than re-serializing the struct
/// because serde_json's output shape is stable-in-practice (struct
/// declaration order, `BTreeMap` for maps) but not a documented
/// guarantee; signing the bytes we actually wrote removes any
/// canonicalization tail-risk.
fn hmac_envelope_into(mac: &mut HmacSha256, sha256sums: &[u8], index_json: &[u8]) {
    mac.update(&(sha256sums.len() as u64).to_be_bytes());
    mac.update(sha256sums);
    mac.update(&(index_json.len() as u64).to_be_bytes());
    mac.update(index_json);
}

/// Sign `SHA256SUMS` + `index.json` with HMAC-SHA256 and write `BUNDLE.sig`.
///
/// Must be called after `EvidenceBuilder::finalize()` — both files
/// must be present on disk with their final contents.
pub fn sign_bundle(bundle_dir: &Path, key: &[u8]) -> Result<PathBuf, SigningError> {
    let sha256sums = read_envelope_input(bundle_dir, "SHA256SUMS")?;
    let index_json = read_envelope_input(bundle_dir, "index.json")?;

    let mut mac = HmacSha256::new_from_slice(key).map_err(|e| SigningError::InvalidKey {
        reason: e.to_string(),
    })?;
    hmac_envelope_into(&mut mac, &sha256sums, &index_json);
    let sig_hex = hex::encode(mac.finalize().into_bytes());

    let sig_path = bundle_dir.join("BUNDLE.sig");
    fs::write(&sig_path, &sig_hex).map_err(|source| SigningError::Write {
        path: "BUNDLE.sig".to_string(),
        source,
    })?;
    Ok(sig_path)
}

/// Verify the HMAC signature in `BUNDLE.sig` against the
/// `SHA256SUMS` + `index.json` envelope.
///
/// Returns `Ok(true)` if valid, `Ok(false)` if invalid, or an error on I/O failure.
pub fn verify_bundle_signature(bundle_dir: &Path, key: &[u8]) -> Result<bool, SigningError> {
    let sha256sums = read_envelope_input(bundle_dir, "SHA256SUMS")?;
    let index_json = read_envelope_input(bundle_dir, "index.json")?;

    let sig_hex =
        fs::read_to_string(bundle_dir.join("BUNDLE.sig")).map_err(|source| SigningError::Read {
            path: "BUNDLE.sig".to_string(),
            source,
        })?;
    let expected = hex::decode(sig_hex.trim()).map_err(SigningError::InvalidSignatureHex)?;

    let mut mac = HmacSha256::new_from_slice(key).map_err(|e| SigningError::InvalidKey {
        reason: e.to_string(),
    })?;
    hmac_envelope_into(&mut mac, &sha256sums, &index_json);

    Ok(mac.verify_slice(&expected).is_ok())
}

fn read_envelope_input(bundle_dir: &Path, filename: &str) -> Result<Vec<u8>, SigningError> {
    fs::read(bundle_dir.join(filename)).map_err(|source| SigningError::Read {
        path: filename.to_string(),
        source,
    })
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
    fn test_hmac_sign_and_verify() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("SHA256SUMS"), "abc123  file.txt\n").unwrap();
        fs::write(dir.path().join("index.json"), b"{\"content_hash\":\"x\"}\n").unwrap();

        let key = b"test-secret-key-bytes";
        let sig_path = sign_bundle(dir.path(), key).unwrap();
        assert!(sig_path.exists());

        // Verify with correct key
        assert!(verify_bundle_signature(dir.path(), key).unwrap());

        // Verify with wrong key
        assert!(!verify_bundle_signature(dir.path(), b"wrong-key").unwrap());
    }

    #[test]
    fn test_hmac_detects_tamper_on_index_json() {
        // A holder without the key cannot edit index.json without
        // rotating BUNDLE.sig — that's the whole point of folding
        // index.json into the HMAC envelope.
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("SHA256SUMS"), "abc123  file.txt\n").unwrap();
        fs::write(
            dir.path().join("index.json"),
            b"{\"engine_git_sha\":\"aaa\"}\n",
        )
        .unwrap();

        let key = b"test-secret-key-bytes";
        sign_bundle(dir.path(), key).unwrap();

        // Tamper index.json: flip engine_git_sha. SHA256SUMS still
        // hashes the content layer fine (index.json isn't in it),
        // but the envelope's second input changed.
        fs::write(
            dir.path().join("index.json"),
            b"{\"engine_git_sha\":\"bbb\"}\n",
        )
        .unwrap();
        assert!(
            !verify_bundle_signature(dir.path(), key).unwrap(),
            "tampered index.json must break HMAC verification"
        );
    }

    #[test]
    fn test_hmac_detects_tamper_on_sha256sums() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("SHA256SUMS"), "abc123  file.txt\n").unwrap();
        fs::write(dir.path().join("index.json"), b"{}\n").unwrap();

        let key = b"test-secret-key-bytes";
        sign_bundle(dir.path(), key).unwrap();

        // Tamper SHA256SUMS.
        fs::write(dir.path().join("SHA256SUMS"), "deadbeef  file.txt\n").unwrap();
        assert!(
            !verify_bundle_signature(dir.path(), key).unwrap(),
            "tampered SHA256SUMS must break HMAC verification"
        );
    }
}
