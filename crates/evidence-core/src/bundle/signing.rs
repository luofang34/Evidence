//! Ed25519 detached signature over a bundle's `SHA256SUMS` + `index.json`
//! envelope.
//!
//! The verifying party needs only the public verifying key (32 bytes); the
//! signing party holds the private signing key (32 bytes of seed material).
//! Together they cover the metadata-layer integrity gap left by `SHA256SUMS`
//! — which deliberately excludes `index.json` to break the self-referential
//! `content_hash` cycle. Editing any field in `index.json` (`engine_git_sha`,
//! `dal_map`, `test_summary`, `trace_outputs` paths, schema versions,
//! timestamps...) without re-signing rotates the verification result to false.
//!
//! This is the second of three integrity layers documented under SYS-001:
//!
//! 1. **Content layer.** Anyone with a SHA-256 implementation confirms
//!    `SHA256SUMS` against the bundle. No key required.
//! 2. **Metadata layer.** *(this module)* Anyone holding the supplier's
//!    32-byte public verifying key confirms `BUNDLE.sig` against the
//!    length-prefixed `(SHA256SUMS, index.json)` envelope. The public key
//!    is a 64-character hex string, distributable in the source repo, in
//!    a release asset, or in a transparency log.
//! 3. **Provenance layer.** Verification implies non-repudiation: only the
//!    holder of the corresponding 32-byte private signing key could have
//!    produced the matching signature.
//!
//! See `cert/QUALIFICATION.md` "Integrity layers" for the auditor framing.

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand_core::{OsRng, TryRngCore};
use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::diagnostic::{DiagnosticCode, Location, Severity};

/// Errors returned by the signing API:
/// [`sign_bundle`] / [`verify_bundle_signature`] / the on-disk key helpers.
#[derive(Debug, Error)]
pub enum SigningError {
    /// Failed to read one of the envelope inputs, the signature file, or a
    /// key file.
    #[error("reading {path}")]
    Read {
        /// Filename being read (bundle-relative for envelope inputs and the
        /// signature; absolute for key paths supplied by the caller).
        path: String,
        /// Underlying OS error.
        #[source]
        source: std::io::Error,
    },
    /// Failed to write `BUNDLE.sig` or a key file.
    #[error("writing {path}")]
    Write {
        /// Filename being written.
        path: String,
        /// Underlying OS error.
        #[source]
        source: std::io::Error,
    },
    /// Key material had the wrong byte length, contained invalid hex, or
    /// failed ed25519 point-decoding.
    #[error("invalid key material: {reason}")]
    InvalidKey {
        /// Human-readable reason (length mismatch, bad hex, point decoding
        /// failure).
        reason: String,
    },
    /// `BUNDLE.sig` contained non-hex bytes.
    #[error("BUNDLE.sig contains invalid hex")]
    InvalidSignatureHex(#[source] hex::FromHexError),
    /// The operating system's randomness source was unavailable while
    /// drawing the seed for a fresh signing key. On supported platforms
    /// this effectively never occurs; the OS entropy call is fallible at
    /// the type level, so the failure is propagated rather than aborting
    /// the process.
    #[error("OS randomness unavailable while generating a signing key")]
    KeygenEntropy {
        /// Underlying `getrandom` failure surfaced by `rand_core::OsRng`.
        #[source]
        source: rand_core::OsError,
    },
}

impl DiagnosticCode for SigningError {
    fn code(&self) -> &'static str {
        match self {
            SigningError::Read { .. } => "SIGN_READ_FAILED",
            SigningError::Write { .. } => "SIGN_WRITE_FAILED",
            SigningError::InvalidKey { .. } => "SIGN_INVALID_KEY",
            SigningError::InvalidSignatureHex(_) => "SIGN_INVALID_SIGNATURE_HEX",
            SigningError::KeygenEntropy { .. } => "SIGN_KEYGEN_ENTROPY",
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
            SigningError::InvalidKey { .. }
            | SigningError::InvalidSignatureHex(_)
            | SigningError::KeygenEntropy { .. } => None,
        };
        file.map(|file| Location {
            file: Some(file),
            ..Location::default()
        })
    }
}

/// Generate a fresh ed25519 signing keypair using OS randomness.
///
/// The returned [`SigningKey`] both signs and exposes its companion
/// [`VerifyingKey`] via `signing_key.verifying_key()`.
///
/// # Errors
///
/// Returns [`SigningError::KeygenEntropy`] if the OS randomness source is
/// unavailable while drawing the 32-byte seed. This is effectively
/// unreachable on supported platforms but is surfaced rather than
/// panicked on, since the entropy call is fallible at the type level.
pub fn generate_signing_key() -> Result<SigningKey, SigningError> {
    // Mirror `SigningKey::generate`: fill 32 bytes from the CSPRNG and
    // build the key from that seed. Drawing the seed explicitly lets the
    // fallible OS-entropy result be propagated instead of unwrapped —
    // the workspace lints forbid unwrap/panic in library code.
    let mut seed = [0u8; 32];
    OsRng
        .try_fill_bytes(&mut seed)
        .map_err(|source| SigningError::KeygenEntropy { source })?;
    Ok(SigningKey::from_bytes(&seed))
}

/// Read an ed25519 signing (private) key from a 64-character hex file.
///
/// Whitespace at the start and end is trimmed before decoding so the file
/// can carry a trailing newline. Other whitespace (interior spaces, line
/// breaks) is rejected — this catches accidental concatenation of two
/// keys or pasted PEM bundles.
pub fn read_signing_key(path: &Path) -> Result<SigningKey, SigningError> {
    let bytes = read_key_bytes::<32>(path)?;
    Ok(SigningKey::from_bytes(&bytes))
}

/// Read an ed25519 verifying (public) key from a 64-character hex file.
pub fn read_verifying_key(path: &Path) -> Result<VerifyingKey, SigningError> {
    let bytes = read_key_bytes::<32>(path)?;
    VerifyingKey::from_bytes(&bytes).map_err(|e| SigningError::InvalidKey {
        reason: format!("ed25519 public-key decoding failed: {e}"),
    })
}

/// Write a signing (private) key as 64-character ASCII hex with a trailing
/// newline.
///
/// The caller is responsible for filesystem permissions on the resulting
/// file (private keys should not be world-readable). The CLI's `keygen`
/// subcommand attempts a best-effort `chmod 600` on Unix.
pub fn write_signing_key(path: &Path, key: &SigningKey) -> Result<(), SigningError> {
    write_hex(path, key.to_bytes().as_ref())
}

/// Write a verifying (public) key as 64-character ASCII hex with a trailing
/// newline.
pub fn write_verifying_key(path: &Path, key: &VerifyingKey) -> Result<(), SigningError> {
    write_hex(path, key.to_bytes().as_ref())
}

fn read_key_bytes<const N: usize>(path: &Path) -> Result<[u8; N], SigningError> {
    let raw = fs::read_to_string(path).map_err(|source| SigningError::Read {
        path: path.display().to_string(),
        source,
    })?;
    let trimmed = raw.trim();
    let decoded = hex::decode(trimmed).map_err(|e| SigningError::InvalidKey {
        reason: format!("expected {} bytes of ASCII hex, hex decoder said: {e}", N),
    })?;
    decoded
        .as_slice()
        .try_into()
        .map_err(|_| SigningError::InvalidKey {
            reason: format!(
                "expected {} bytes of ed25519 key material, got {}",
                N,
                decoded.len()
            ),
        })
}

fn write_hex(path: &Path, bytes: &[u8]) -> Result<(), SigningError> {
    let mut hex_str = hex::encode(bytes);
    hex_str.push('\n');
    fs::write(path, hex_str).map_err(|source| SigningError::Write {
        path: path.display().to_string(),
        source,
    })
}

/// Length-prefixed two-input envelope: `u64_be(|sha256sums|) || sha256sums
/// || u64_be(|index_json|) || index_json`. The framing prevents `(A, B)`
/// from colliding with any `(A', B')` whose concatenation happens to share
/// the same byte stream.
///
/// Disk bytes are signed verbatim rather than re-serialized — `serde_json`'s
/// output shape is stable in practice (struct field order, `BTreeMap` for
/// maps) but not a documented guarantee, and signing the bytes we actually
/// wrote eliminates any canonicalization tail-risk.
fn envelope_bytes(sha256sums: &[u8], index_json: &[u8]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(16 + sha256sums.len() + index_json.len());
    buf.extend_from_slice(&(sha256sums.len() as u64).to_be_bytes());
    buf.extend_from_slice(sha256sums);
    buf.extend_from_slice(&(index_json.len() as u64).to_be_bytes());
    buf.extend_from_slice(index_json);
    buf
}

/// Sign the bundle's `SHA256SUMS` + `index.json` envelope with the supplied
/// ed25519 signing key. Writes the 64-byte signature as a 128-character hex
/// string with trailing newline to `BUNDLE.sig` and returns its path.
///
/// Must be called after `EvidenceBuilder::finalize()` — both envelope
/// inputs must be present on disk in their final form.
pub fn sign_bundle(bundle_dir: &Path, key: &SigningKey) -> Result<PathBuf, SigningError> {
    let sha256sums = read_envelope_input(bundle_dir, "SHA256SUMS")?;
    let index_json = read_envelope_input(bundle_dir, "index.json")?;
    let envelope = envelope_bytes(&sha256sums, &index_json);
    let signature: Signature = key.sign(&envelope);

    let mut sig_hex = hex::encode(signature.to_bytes());
    sig_hex.push('\n');

    let sig_path = bundle_dir.join("BUNDLE.sig");
    fs::write(&sig_path, &sig_hex).map_err(|source| SigningError::Write {
        path: "BUNDLE.sig".to_string(),
        source,
    })?;
    Ok(sig_path)
}

/// Verify the detached ed25519 signature in `BUNDLE.sig` against the
/// `SHA256SUMS` + `index.json` envelope.
///
/// Returns `Ok(true)` for a valid signature, `Ok(false)` for a syntactically
/// well-formed but cryptographically invalid one, or an error if any of the
/// inputs cannot be read or decoded.
pub fn verify_bundle_signature(
    bundle_dir: &Path,
    key: &VerifyingKey,
) -> Result<bool, SigningError> {
    let sha256sums = read_envelope_input(bundle_dir, "SHA256SUMS")?;
    let index_json = read_envelope_input(bundle_dir, "index.json")?;

    let sig_text =
        fs::read_to_string(bundle_dir.join("BUNDLE.sig")).map_err(|source| SigningError::Read {
            path: "BUNDLE.sig".to_string(),
            source,
        })?;
    let sig_bytes_vec = hex::decode(sig_text.trim()).map_err(SigningError::InvalidSignatureHex)?;
    let sig_bytes: [u8; 64] =
        sig_bytes_vec
            .as_slice()
            .try_into()
            .map_err(|_| SigningError::InvalidKey {
                reason: format!(
                    "ed25519 signature must be 64 bytes (got {})",
                    sig_bytes_vec.len()
                ),
            })?;
    let signature = Signature::from_bytes(&sig_bytes);

    let envelope = envelope_bytes(&sha256sums, &index_json);
    Ok(key.verify(&envelope, &signature).is_ok())
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

    fn deterministic_signing_key(seed_byte: u8) -> SigningKey {
        SigningKey::from_bytes(&[seed_byte; 32])
    }

    #[test]
    fn sign_and_verify_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("SHA256SUMS"), "abc123  file.txt\n").unwrap();
        fs::write(dir.path().join("index.json"), b"{\"content_hash\":\"x\"}\n").unwrap();

        let signing = deterministic_signing_key(7);
        let verifying = signing.verifying_key();

        let sig_path = sign_bundle(dir.path(), &signing).unwrap();
        assert!(sig_path.exists());

        assert!(verify_bundle_signature(dir.path(), &verifying).unwrap());
    }

    #[test]
    fn verify_rejects_signature_made_with_different_key() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("SHA256SUMS"), "abc123  file.txt\n").unwrap();
        fs::write(dir.path().join("index.json"), b"{}\n").unwrap();

        let supplier = deterministic_signing_key(7);
        sign_bundle(dir.path(), &supplier).unwrap();

        let attacker_pubkey = deterministic_signing_key(8).verifying_key();
        assert!(
            !verify_bundle_signature(dir.path(), &attacker_pubkey).unwrap(),
            "verifying with the wrong public key must fail"
        );
    }

    #[test]
    fn verify_rejects_index_json_tamper() {
        // index.json is excluded from SHA256SUMS by design (the
        // self-referential content_hash cycle). The signature envelope
        // closes that gap: editing any index field rotates the
        // verification result.
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("SHA256SUMS"), "abc123  file.txt\n").unwrap();
        fs::write(
            dir.path().join("index.json"),
            b"{\"engine_git_sha\":\"aaa\"}\n",
        )
        .unwrap();

        let signing = deterministic_signing_key(7);
        sign_bundle(dir.path(), &signing).unwrap();

        fs::write(
            dir.path().join("index.json"),
            b"{\"engine_git_sha\":\"bbb\"}\n",
        )
        .unwrap();
        assert!(
            !verify_bundle_signature(dir.path(), &signing.verifying_key()).unwrap(),
            "tampered index.json must break signature verification"
        );
    }

    #[test]
    fn verify_rejects_sha256sums_tamper() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("SHA256SUMS"), "abc123  file.txt\n").unwrap();
        fs::write(dir.path().join("index.json"), b"{}\n").unwrap();

        let signing = deterministic_signing_key(7);
        sign_bundle(dir.path(), &signing).unwrap();

        fs::write(dir.path().join("SHA256SUMS"), "deadbeef  file.txt\n").unwrap();
        assert!(
            !verify_bundle_signature(dir.path(), &signing.verifying_key()).unwrap(),
            "tampered SHA256SUMS must break signature verification"
        );
    }

    #[test]
    fn read_write_signing_key_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("signing.key");
        let original = deterministic_signing_key(42);

        write_signing_key(&path, &original).unwrap();
        let recovered = read_signing_key(&path).unwrap();

        assert_eq!(original.to_bytes(), recovered.to_bytes());
    }

    #[test]
    fn read_write_verifying_key_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("signing.pub");
        let original = deterministic_signing_key(42).verifying_key();

        write_verifying_key(&path, &original).unwrap();
        let recovered = read_verifying_key(&path).unwrap();

        assert_eq!(original.to_bytes(), recovered.to_bytes());
    }

    #[test]
    fn read_signing_key_rejects_short_input() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("signing.key");
        // 31 bytes of zero, hex-encoded -> 62 chars, will decode but fail the length check.
        fs::write(&path, "00".repeat(31)).unwrap();
        let err = read_signing_key(&path).unwrap_err();
        match err {
            SigningError::InvalidKey { reason } => {
                assert!(
                    reason.contains("32"),
                    "reason should name the expected length: {reason}"
                );
            }
            other => panic!("expected InvalidKey, got {other:?}"),
        }
    }

    #[test]
    fn read_signing_key_rejects_non_hex_input() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("signing.key");
        fs::write(&path, "not-hex-content").unwrap();
        let err = read_signing_key(&path).unwrap_err();
        match err {
            SigningError::InvalidKey { .. } => (),
            other => panic!("expected InvalidKey, got {other:?}"),
        }
    }

    #[test]
    fn generate_signing_key_returns_distinct_keys() {
        let a = generate_signing_key().expect("OS entropy available in test");
        let b = generate_signing_key().expect("OS entropy available in test");
        assert_ne!(
            a.to_bytes(),
            b.to_bytes(),
            "OsRng must not produce duplicate keys back-to-back"
        );
    }

    #[test]
    fn sign_bundle_overwrites_existing_signature() {
        // Re-signing replaces the previous BUNDLE.sig in place — there is
        // never a stale signature alongside a fresh one.
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("SHA256SUMS"), "first  a\n").unwrap();
        fs::write(dir.path().join("index.json"), b"{\"v\":1}\n").unwrap();

        let signing = deterministic_signing_key(7);
        sign_bundle(dir.path(), &signing).unwrap();
        let first_sig = fs::read_to_string(dir.path().join("BUNDLE.sig")).unwrap();

        fs::write(dir.path().join("SHA256SUMS"), "second  a\n").unwrap();
        sign_bundle(dir.path(), &signing).unwrap();
        let second_sig = fs::read_to_string(dir.path().join("BUNDLE.sig")).unwrap();

        assert_ne!(first_sig, second_sig);
    }
}
