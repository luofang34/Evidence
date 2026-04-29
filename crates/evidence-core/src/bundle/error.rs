//! `BuilderError` — the error type returned from every
//! `EvidenceBuilder` method.
//!
//! Wraps the leaf error types of the modules `EvidenceBuilder`
//! orchestrates (git, hash), plus its own bundle-lifecycle variants
//! (dirty tree, TOCTOU, existing directory, I/O on bundle-internal
//! paths).

use std::path::PathBuf;

use thiserror::Error;

use crate::diagnostic::{DiagnosticCode, Location, Severity};
use crate::git::GitError;
use crate::hash::HashError;
use crate::policy::Profile;

/// Errors returned by [`crate::bundle::EvidenceBuilder`] methods.
#[derive(Debug, Error)]
pub enum BuilderError {
    /// A git operation failed (snapshot, shallow-clone check, TOCTOU
    /// re-read, etc.).
    #[error(transparent)]
    Git(#[from] GitError),
    /// Content-layer file hashing failed.
    #[error(transparent)]
    Hash(#[from] HashError),
    /// A cert/record profile run started against a dirty working tree.
    #[error("profile '{profile}' requires clean git tree{suffix}")]
    DirtyGitTree {
        /// Profile that required a clean tree.
        profile: Profile,
        /// Dirty-files listing + remediation recipe. Empty when
        /// `git status` couldn't enumerate files.
        suffix: String,
    },
    /// A previous evidence run left a bundle directory at the same
    /// path. Overwriting is deliberately refused so no prior bundle
    /// is silently clobbered.
    #[error(
        "Bundle directory {path} already exists. Remove it first or use a different --out-dir."
    )]
    BundleExists {
        /// Path that already exists.
        path: PathBuf,
    },
    /// `fs::create_dir_all` / `fs::write` / `fs::read` failed on a
    /// bundle-internal path.
    #[error("{op} {path}")]
    Io {
        /// Verb describing the I/O step (`creating`, `writing`, `reading`).
        op: &'static str,
        /// Path the I/O step was targeting.
        path: PathBuf,
        /// Underlying OS error.
        #[source]
        source: std::io::Error,
    },
    /// `Command::output()` failed to launch a sub-process.
    #[error("running {display_name}")]
    RunCommand {
        /// Short name the builder uses for the subprocess
        /// (e.g. `"cargo test --workspace"`).
        display_name: String,
        /// Underlying spawn error.
        #[source]
        source: std::io::Error,
    },
    /// `std::env::current_dir()` failed; run_capture couldn't record
    /// the CWD of the subprocess it was about to launch.
    #[error("reading current working directory for command capture")]
    CurrentDir(#[source] std::io::Error),
    /// `serde_json::to_vec_pretty` / `from_slice` failed on a bundle
    /// JSON file.
    #[error("serializing {kind} JSON")]
    Serialize {
        /// Label of the file being (de)serialized.
        kind: &'static str,
        /// Underlying serde_json error.
        #[source]
        source: serde_json::Error,
    },
    /// Parsing env.json to derive the deterministic manifest failed.
    #[error("parsing env.json to derive deterministic manifest")]
    ParseEnv(#[source] serde_json::Error),
    /// Git HEAD moved between builder construction and `finalize()`.
    /// The source tree may have changed; the bundle cannot faithfully
    /// represent "the commit snapshotted at start".
    #[error(
        "TOCTOU: git HEAD changed during evidence generation.\n\
         Snapshot SHA: {snapshot_sha}\n\
         Current SHA:  {current_sha}\n\
         Source files may have changed. Re-run evidence generation."
    )]
    Toctou {
        /// SHA captured at `EvidenceBuilder::new` time.
        snapshot_sha: String,
        /// SHA observed at `finalize()` time.
        current_sha: String,
    },
    /// `cargo metadata --format-version 1` failed when the builder
    /// tried to write `cargo_metadata.json` into the bundle. Distinct
    /// from `RunCommand` so the diagnostic carries
    /// `BUNDLE_CARGO_METADATA_FAILED` rather than the generic run-
    /// command code — the Layer-3 verify-time recheck can't proceed
    /// without the artifact.
    #[error("running `cargo metadata` for cargo_metadata.json artifact")]
    CargoMetadataRun(#[source] crate::util::CmdError),
    /// `CargoMetadataProjection::from_raw_metadata` failed on the
    /// output of `cargo metadata`. Same diagnostic-code rationale as
    /// `CargoMetadataRun`.
    #[error("parsing cargo metadata for projection")]
    CargoMetadataProject(#[source] crate::cargo_metadata::ProjectionError),
}

impl DiagnosticCode for BuilderError {
    // `#[rustfmt::skip]` keeps the merged variant arm on a single
    // line — the `diagnostic_codes_locked` walker matches
    // `=> "CODE"` directly and doesn't follow `=> { "CODE" }` block
    // forms. Two-variant or-patterns wrap into block form by default,
    // so the attribute pins single-line form for every arm.
    #[rustfmt::skip]
    fn code(&self) -> &'static str {
        // `Git(_)` and `Hash(_)` keep their own BUNDLE_* codes rather
        // than forwarding to the inner error's code; wrapping them
        // here preserves the "which phase of the builder surfaced
        // this?" signal that agents care about. Inner detail is still
        // reachable via `std::error::Error::source()`.
        match self {
            BuilderError::Git(_)                                                   => "BUNDLE_GIT_FAILED",
            BuilderError::Hash(_)                                                  => "BUNDLE_HASH_FAILED",
            BuilderError::DirtyGitTree { .. }                                      => "BUNDLE_DIRTY_GIT_TREE",
            BuilderError::BundleExists { .. }                                      => "BUNDLE_ALREADY_EXISTS",
            BuilderError::Io { .. }                                                => "BUNDLE_IO_FAILED",
            BuilderError::RunCommand { .. }                                        => "BUNDLE_RUN_COMMAND_FAILED",
            BuilderError::CurrentDir(_)                                            => "BUNDLE_CURRENT_DIR_FAILED",
            BuilderError::Serialize { .. }                                         => "BUNDLE_SERIALIZE_FAILED",
            BuilderError::ParseEnv(_)                                              => "BUNDLE_PARSE_ENV_FAILED",
            BuilderError::Toctou { .. }                                            => "BUNDLE_TOCTOU",
            BuilderError::CargoMetadataRun(_) | BuilderError::CargoMetadataProject(_) => "BUNDLE_CARGO_METADATA_FAILED",
        }
    }

    fn severity(&self) -> Severity {
        Severity::Error
    }

    fn location(&self) -> Option<Location> {
        let file = match self {
            BuilderError::BundleExists { path } | BuilderError::Io { path, .. } => {
                Some(path.clone())
            }
            BuilderError::Git(_)
            | BuilderError::Hash(_)
            | BuilderError::DirtyGitTree { .. }
            | BuilderError::RunCommand { .. }
            | BuilderError::CurrentDir(_)
            | BuilderError::Serialize { .. }
            | BuilderError::ParseEnv(_)
            | BuilderError::Toctou { .. }
            | BuilderError::CargoMetadataRun(_)
            | BuilderError::CargoMetadataProject(_) => None,
        };
        file.map(|file| Location {
            file: Some(file),
            ..Location::default()
        })
    }
}
