//! Verify-key path resolution and load-error classification.
//!
//! `cargo evidence verify` accepts an optional `--verify-key`. When
//! the flag is absent, the CLI walks a fixed priority list to fall
//! back to the project's committed public-key anchor:
//!
//! 1. `--verify-key <path>` — explicit user choice; always honored.
//! 2. `$EVIDENCE_VERIFY_KEY_PATH` — CI's documented signing-secret
//!    injection point; treated as explicit.
//! 3. `cert/signing.pub` — the committed in-repo anchor. Only
//!    consulted when the bundle actually has a `BUNDLE.sig` to
//!    verify; otherwise we'd turn every dev-profile bundle that
//!    happens to live next to a project repo into a verify failure
//!    just because the workspace publishes a public key. The
//!    unsigned-bundle path stays a no-op without explicit opt-in.
//!
//! Load failures are split between I/O (`VERIFY_RUNTIME_READ_VERIFY_KEY`,
//! a documented diagnostic) and parse errors (`SIGN_INVALID_KEY`
//! etc.) so an agent gets a structurally distinct code for each.

use std::path::{Path, PathBuf};

use evidence_core::SigningError;
use evidence_core::diagnostic::{Diagnostic, DiagnosticCode};
use evidence_core::verify::VerifyRuntimeError;

/// Walk the resolution priority described at the module top.
///
/// Returns `None` when no key resolves (no flag, no env var, and either
/// no anchor file present **or** no `BUNDLE.sig` to verify against).
/// Explicit `--verify-key` and the env-var hook always win, regardless
/// of `BUNDLE.sig` presence — those are user-opt-in surfaces and
/// "I asked you to verify but the bundle has no signature" should
/// remain a verification failure, not a silent skip.
pub(super) fn resolve_verify_key_path(
    explicit: Option<PathBuf>,
    bundle_path: &Path,
) -> Option<PathBuf> {
    if let Some(p) = explicit {
        return Some(p);
    }
    if let Ok(env) = std::env::var("EVIDENCE_VERIFY_KEY_PATH") {
        if !env.is_empty() {
            return Some(PathBuf::from(env));
        }
    }
    // Default-anchor branch: only consult `cert/signing.pub` when the
    // bundle actually carries a signature. Otherwise we'd convert the
    // mere existence of a project-anchor file in CWD into a per-bundle
    // verification failure — hostile to dev-profile workflows that
    // legitimately ship unsigned bundles.
    if !bundle_path.join("BUNDLE.sig").exists() {
        return None;
    }
    let default = PathBuf::from("cert/signing.pub");
    default.exists().then_some(default)
}

/// Classify a verify-key load failure into a structured diagnostic.
/// I/O failures route through `VerifyRuntimeError::ReadVerifyKey` so
/// the documented `VERIFY_RUNTIME_READ_VERIFY_KEY` surface is
/// preserved; parse failures keep the underlying `SigningError`'s
/// code (`SIGN_INVALID_KEY` etc.). Callers needing the human-readable
/// text pull it from `diagnostic.message`.
pub(super) fn classify_key_load_diagnostic(path: &Path, err: SigningError) -> Diagnostic {
    match err {
        SigningError::Read { source, .. } => VerifyRuntimeError::ReadVerifyKey {
            path: path.to_path_buf(),
            source,
        }
        .to_diagnostic(),
        other => other.to_diagnostic(),
    }
}
