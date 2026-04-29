//! `VerifyRuntimeError` + its `DiagnosticCode` impl.
//!
//! Catastrophic errors that abort verification before per-check
//! accumulation begins. Distinct from
//! [`crate::verify::VerifyError`]: a `VerifyError` records a
//! *validation finding* (a bundle field or hash disagrees);
//! `VerifyRuntimeError` records an *I/O or parsing fault* where
//! the verifier can't even get far enough to make a finding.
//!
//! Pulled out of `bundle.rs` so the orchestrator stays under the
//! workspace 500-line file-size limit. Each variant maps to a
//! `VERIFY_RUNTIME_*` code claimed by `LLR-069` (key-read path)
//! and earlier LLRs.

use std::path::PathBuf;

use thiserror::Error;

use crate::bundle::SigningError;
use crate::diagnostic::{DiagnosticCode, Location, Severity};
use crate::hash::HashError;

/// Catastrophic errors that abort verification before per-check
/// accumulation begins.
#[derive(Debug, Error)]
pub enum VerifyRuntimeError {
    /// Bundle path does not exist or is not a directory.
    #[error("Bundle path does not exist or is not a directory: {0}")]
    BundleNotFound(PathBuf),
    /// I/O failure reading a bundle file.
    #[error("reading {path}")]
    ReadFile {
        /// Bundle-relative path whose read failed.
        path: PathBuf,
        /// Underlying OS error.
        #[source]
        source: std::io::Error,
    },
    /// I/O failure reading the `--verify-key` file. Distinct from
    /// [`VerifyRuntimeError::ReadFile`] (which reads a path inside
    /// the bundle) — this read happens before any bundle file is
    /// touched, so the diagnostic carries the user-supplied key
    /// path rather than a bundle-relative path.
    #[error("reading verify key from {path}")]
    ReadVerifyKey {
        /// User-supplied path passed via `--verify-key`.
        path: PathBuf,
        /// Underlying OS error.
        #[source]
        source: std::io::Error,
    },
    /// `index.json` failed to parse as JSON.
    #[error("parsing index.json")]
    ParseIndex(#[source] serde_json::Error),
    /// File tree walk raised an error (bundle contained an
    /// unreadable directory or symlink).
    #[error("walking bundle tree")]
    Walk(#[source] walkdir::Error),
    /// A file hash couldn't be computed.
    #[error(transparent)]
    Hash(#[from] HashError),
    /// HMAC signature verification had an I/O or envelope error
    /// (distinct from the signature being invalid, which is a
    /// [`crate::verify::VerifyError::HmacFailure`]).
    #[error(transparent)]
    Signing(#[from] SigningError),
}

impl DiagnosticCode for VerifyRuntimeError {
    fn code(&self) -> &'static str {
        // Runtime faults that abort verify before it can make any
        // findings. These map to exit code 1 (not 2), per the
        // Schema Rule 1 contract.
        match self {
            VerifyRuntimeError::BundleNotFound(_) => "VERIFY_RUNTIME_BUNDLE_NOT_FOUND",
            VerifyRuntimeError::ReadFile { .. } => "VERIFY_RUNTIME_READ_FILE",
            VerifyRuntimeError::ReadVerifyKey { .. } => "VERIFY_RUNTIME_READ_VERIFY_KEY",
            VerifyRuntimeError::ParseIndex(_) => "VERIFY_RUNTIME_PARSE_INDEX",
            VerifyRuntimeError::Walk(_) => "VERIFY_RUNTIME_WALK",
            VerifyRuntimeError::Hash(_) => "VERIFY_RUNTIME_HASH",
            VerifyRuntimeError::Signing(_) => "VERIFY_RUNTIME_SIGNING",
        }
    }

    fn severity(&self) -> Severity {
        Severity::Error
    }

    fn location(&self) -> Option<Location> {
        match self {
            VerifyRuntimeError::BundleNotFound(p) => Some(Location {
                file: Some(p.clone()),
                ..Location::default()
            }),
            VerifyRuntimeError::ReadFile { path, .. } => Some(Location {
                file: Some(path.clone()),
                ..Location::default()
            }),
            VerifyRuntimeError::ReadVerifyKey { path, .. } => Some(Location {
                file: Some(path.clone()),
                ..Location::default()
            }),
            // Remaining variants wrap inner errors without surfacing
            // a path at this layer. `Hash(HashError)` and friends
            // would grow their own `DiagnosticCode` impls and the
            // wrapper would then forward via `source()`.
            VerifyRuntimeError::ParseIndex(_)
            | VerifyRuntimeError::Walk(_)
            | VerifyRuntimeError::Hash(_)
            | VerifyRuntimeError::Signing(_) => None,
        }
    }
}

#[cfg(test)]
#[path = "runtime_error/tests.rs"]
mod tests;
