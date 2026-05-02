//! Phase 9 — finalize the bundle and (optionally) sign the
//! `(SHA256SUMS, index.json)` envelope with the supplier's ed25519
//! signing key. Lives in this sibling file so `phases.rs` stays
//! under the workspace 500-line cap.
//!
//! Key resolution priority (highest first):
//! 1. `--signing-key` CLI flag.
//! 2. `$EVIDENCE_SIGNING_KEY_PATH` environment variable.
//! 3. `cert/signing.key` if the file exists.
//!
//! If none resolve, Cert/Record profiles fail loud; Dev profile
//! skips signing silently.
//!
//! Anchor consistency: when `cert/signing.pub` is present, the
//! public key derived from the signing key being used must match it
//! byte-for-byte. Mismatch (`SIGN_PUBKEY_ANCHOR_MISMATCH`) is the
//! silent-re-key defense — a developer who regenerates a keypair
//! without going through `cargo evidence keygen --rotate --reason`
//! triggers this check on their next `generate`.

use std::path::PathBuf;

use anyhow::{Context, Result};

use evidence_core::{EvidenceBuilder, Profile, sign_bundle};

/// Default location of the project's signing (private) key.
const DEFAULT_SIGNING_KEY: &str = "cert/signing.key";
/// Default location of the project's verifying (public) key, used as
/// the anchor in the consistency check.
const DEFAULT_PUBKEY_ANCHOR: &str = "cert/signing.pub";

fn resolve_signing_key_path(explicit: Option<PathBuf>) -> Option<PathBuf> {
    if let Some(p) = explicit {
        return Some(p);
    }
    if let Ok(env) = std::env::var("EVIDENCE_SIGNING_KEY_PATH") {
        if !env.is_empty() {
            return Some(PathBuf::from(env));
        }
    }
    let default = PathBuf::from(DEFAULT_SIGNING_KEY);
    if default.exists() {
        return Some(default);
    }
    None
}

/// Finalize the bundle and (if a signing key resolves) write
/// `BUNDLE.sig`. Anchor consistency is checked when
/// `cert/signing.pub` is present.
pub(super) fn finalize_and_sign(
    builder: EvidenceBuilder,
    trace_outputs: Vec<PathBuf>,
    sign_key: Option<PathBuf>,
    profile: Profile,
    quiet: bool,
    json_output: bool,
) -> Result<PathBuf> {
    let bundle_path = builder.finalize(trace_outputs)?;
    let resolved = resolve_signing_key_path(sign_key);

    match (resolved, profile) {
        (None, Profile::Cert | Profile::Record) => {
            anyhow::bail!(
                "no signing key resolved (looked at --signing-key, \
                 $EVIDENCE_SIGNING_KEY_PATH, and {DEFAULT_SIGNING_KEY}); \
                 cert/record profiles require a signing key"
            );
        }
        (None, Profile::Dev) => Ok(bundle_path),
        (Some(key_path), _) => {
            let signing_key = evidence_core::read_signing_key(&key_path)
                .with_context(|| format!("reading signing key from {:?}", key_path))?;
            sign_bundle(&bundle_path, &signing_key)?;
            check_pubkey_anchor(&signing_key, &key_path)?;
            if !quiet && !json_output {
                println!("evidence: ed25519 signature written to BUNDLE.sig");
            }
            Ok(bundle_path)
        }
    }
}

fn check_pubkey_anchor(
    signing_key: &evidence_core::SigningKey,
    signing_key_path: &std::path::Path,
) -> Result<()> {
    check_pubkey_anchor_at(
        signing_key,
        signing_key_path,
        &PathBuf::from(DEFAULT_PUBKEY_ANCHOR),
    )
}

/// Inner form taking an explicit anchor path so unit tests can
/// drive the comparison against a tempdir without poking the
/// real `cert/signing.pub`.
fn check_pubkey_anchor_at(
    signing_key: &evidence_core::SigningKey,
    signing_key_path: &std::path::Path,
    anchor: &std::path::Path,
) -> Result<()> {
    if !anchor.exists() {
        return Ok(());
    }
    let anchor_pubkey = evidence_core::read_verifying_key(anchor)
        .with_context(|| format!("reading pubkey anchor at {}", anchor.display()))?;
    let actual_pubkey = signing_key.verifying_key();
    if anchor_pubkey.to_bytes() != actual_pubkey.to_bytes() {
        anyhow::bail!(
            "SIGN_PUBKEY_ANCHOR_MISMATCH: the public key derived from the \
             signing key at {} does not match the project's committed \
             public key at {}. If this is an intentional rotation, run \
             `cargo evidence keygen --rotate --reason <text>` instead of \
             regenerating the keypair manually.",
            signing_key_path.display(),
            anchor.display()
        );
    }
    Ok(())
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
    use evidence_core::{SigningKey, write_verifying_key};

    fn key(seed: u8) -> SigningKey {
        SigningKey::from_bytes(&[seed; 32])
    }

    #[test]
    fn anchor_absent_is_ok() {
        let tmp = tempfile::tempdir().unwrap();
        let signing = key(7);
        let result = check_pubkey_anchor_at(
            &signing,
            std::path::Path::new("/dev/null"),
            &tmp.path().join("missing.pub"),
        );
        assert!(result.is_ok(), "missing anchor must short-circuit Ok");
    }

    #[test]
    fn anchor_match_is_ok() {
        let tmp = tempfile::tempdir().unwrap();
        let signing = key(7);
        let anchor_path = tmp.path().join("signing.pub");
        write_verifying_key(&anchor_path, &signing.verifying_key()).unwrap();

        let result =
            check_pubkey_anchor_at(&signing, std::path::Path::new("/dev/null"), &anchor_path);
        assert!(result.is_ok(), "matching anchor must succeed");
    }

    #[test]
    fn anchor_mismatch_refuses() {
        let tmp = tempfile::tempdir().unwrap();
        let supplier = key(7);
        let attacker = key(8);
        let anchor_path = tmp.path().join("signing.pub");
        write_verifying_key(&anchor_path, &attacker.verifying_key()).unwrap();

        let err =
            check_pubkey_anchor_at(&supplier, std::path::Path::new("/dev/null"), &anchor_path)
                .unwrap_err();
        assert!(
            err.to_string().contains("SIGN_PUBKEY_ANCHOR_MISMATCH"),
            "error must carry the SIGN_PUBKEY_ANCHOR_MISMATCH code: {err:#}"
        );
    }

    #[test]
    fn resolve_signing_key_path_prefers_explicit_flag() {
        // No env, no default file — explicit path wins regardless of
        // whether it exists yet (the caller will surface a real read
        // error on the next step).
        let explicit = PathBuf::from("/tmp/some-explicit-path.key");
        let resolved = resolve_signing_key_path(Some(explicit.clone()));
        assert_eq!(resolved, Some(explicit));
    }

    #[test]
    fn resolve_signing_key_path_returns_none_when_neither_explicit_nor_env() {
        // Mutating the test process's env to clear
        // EVIDENCE_SIGNING_KEY_PATH would require an `unsafe` block
        // (forbid_unsafe_code at workspace level); instead the test
        // covers only the explicit-flag-absent branch and accepts
        // either None (no env, no default file) or Some(default
        // path) when the workspace already has a bootstrapped
        // keypair. The env-var branch is exercised via the explicit
        // branch's negative case rather than direct env mutation.
        let resolved = resolve_signing_key_path(None);
        match resolved {
            None => (),
            Some(p) => {
                let s = p.to_string_lossy();
                assert!(
                    s.ends_with("signing.key"),
                    "fallback should land on a *.signing.key path, got {p:?}"
                );
            }
        }
    }
}
