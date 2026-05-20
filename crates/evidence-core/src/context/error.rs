//! Typed error enum for the [`context`](super) module.
//!
//! Variants carry the upstream cause via `#[source]` / `#[from]` so
//! context is never lost. The CLI layer (`cargo_evidence::cli::context`)
//! picks the right `CONTEXT_*` diagnostic code per variant — keeping
//! the mapping at the emit-site avoids a `DiagnosticCode` impl whose
//! match arms would collide on Schema Rule 3 (one code per arm) for
//! the IO / manifest variants that share a single content code.

use std::path::PathBuf;

use thiserror::Error;

use crate::trace::TraceReadError;

/// Errors returned by [`resolve_selector`](super::resolver::resolve_selector)
/// and [`context_for`](super::context_for).
#[derive(Debug, Error)]
pub enum ContextError {
    /// The selector did not resolve as a file under
    /// `crates/<crate>/`, a known workspace crate name, or a
    /// reasonably-shaped module path. Caller wrote the input
    /// `selector` in the message verbatim so the agent can fix the
    /// typo.
    #[error("selector '{0}' resolves outside the workspace")]
    SelectorOutOfScope(String),
    /// `cert/trace/` is missing from the workspace — the
    /// non-adopter graceful path. The CLI maps this to
    /// `CONTEXT_NO_TRACE_CONFIGURED` (info) + `CONTEXT_OK` (exit 0)
    /// rather than treating it as an error.
    #[error("no trace configured at {0}")]
    TraceNotConfigured(PathBuf),
    /// Underlying trace TOML read or parse failure. Carries the
    /// upstream `TraceReadError` for path / span context.
    #[error("trace read failed")]
    TraceRead(#[from] TraceReadError),
    /// I/O failure while reading a non-trace file (`Cargo.toml` lookup,
    /// `CLAUDE.md` discovery, etc.).
    #[error("I/O failed")]
    Io(#[from] std::io::Error),
    /// Failed to read a workspace crate's `Cargo.toml`. Carries the
    /// path so the operator knows which manifest tripped the lookup.
    #[error("reading {path}")]
    CargoManifestRead {
        /// Manifest path that failed to read.
        path: PathBuf,
        /// Underlying OS error.
        #[source]
        err: std::io::Error,
    },
    /// Failed to parse a workspace crate's `Cargo.toml`. Carries the
    /// path + the underlying TOML error.
    #[error("parsing {path}")]
    CargoManifestParse {
        /// Manifest path that failed to parse.
        path: PathBuf,
        /// Underlying TOML error.
        #[source]
        err: toml::de::Error,
    },
}

impl ContextError {
    /// Pick the `CONTEXT_*` content code the CLI emits for this
    /// variant. Kept outside the [`DiagnosticCode`] trait because the
    /// runtime variants (`TraceRead`, `Io`, `CargoManifestRead`,
    /// `CargoManifestParse`) share a single content code
    /// (`CONTEXT_RUNTIME_ERROR`); a trait impl with multiple match
    /// arms returning the same string would trip the Schema Rule 3
    /// uniqueness check.
    ///
    /// Runtime / I/O faults are deliberately distinct from
    /// `CONTEXT_SELECTOR_OUT_OF_SCOPE` so an agent parsing the JSONL
    /// stream can tell the difference between a typo'd selector
    /// (user fixable) and a tool-side failure (e.g. unreadable
    /// `Cargo.toml`, missing permissions).
    ///
    /// [`DiagnosticCode`]: crate::diagnostic::DiagnosticCode
    pub fn content_code(&self) -> &'static str {
        match self {
            ContextError::SelectorOutOfScope(_) => "CONTEXT_SELECTOR_OUT_OF_SCOPE",
            ContextError::TraceNotConfigured(_) => "CONTEXT_NO_TRACE_CONFIGURED",
            ContextError::TraceRead(_)
            | ContextError::Io(_)
            | ContextError::CargoManifestRead { .. }
            | ContextError::CargoManifestParse { .. } => "CONTEXT_RUNTIME_ERROR",
        }
    }
}
