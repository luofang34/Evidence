//! Verify-key path resolution and load-error classification.
//!
//! `cargo evidence verify` accepts an optional `--verify-key`. When
//! the flag is absent, the CLI walks a fixed priority list to fall
//! back to the project's committed public-key anchor:
//!
//! 1. `--verify-key <path>` — explicit user choice.
//! 2. `$EVIDENCE_VERIFY_KEY_PATH` — useful for CI that injects a
//!    key without rewriting every downstream `verify` invocation.
//! 3. `cert/signing.pub` — the committed in-repo anchor; makes the
//!    no-config trust root visible in source control.
//!
//! Load failures are split between I/O (`VERIFY_RUNTIME_READ_VERIFY_KEY`,
//! a documented diagnostic) and parse errors (`SIGN_INVALID_KEY`
//! etc.) so an agent gets a structurally distinct code for each.

use std::path::{Path, PathBuf};

use evidence_core::SigningError;
use evidence_core::diagnostic::{Diagnostic, DiagnosticCode};
use evidence_core::verify::VerifyRuntimeError;

/// Walk the resolution priority described at the module top. Returns
/// `None` only when none of the three sources resolve.
pub(super) fn resolve_verify_key_path(explicit: Option<PathBuf>) -> Option<PathBuf> {
    if let Some(p) = explicit {
        return Some(p);
    }
    if let Ok(env) = std::env::var("EVIDENCE_VERIFY_KEY_PATH") {
        if !env.is_empty() {
            return Some(PathBuf::from(env));
        }
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
