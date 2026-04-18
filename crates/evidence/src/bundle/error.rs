//! `BuilderError` — the error type returned from every
//! `EvidenceBuilder` method.
//!
//! Wraps the leaf error types of the modules `EvidenceBuilder`
//! orchestrates (git, hash), plus its own bundle-lifecycle variants
//! (dirty tree, TOCTOU, existing directory, I/O on bundle-internal
//! paths).

use std::path::PathBuf;

use thiserror::Error;

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
    DirtyGitTree { profile: Profile, suffix: String },
    /// A previous evidence run left a bundle directory at the same
    /// path. Overwriting is deliberately refused so no prior bundle
    /// is silently clobbered.
    #[error(
        "Bundle directory {path} already exists. Remove it first or use a different --out-dir."
    )]
    BundleExists { path: PathBuf },
    /// `fs::create_dir_all` / `fs::write` / `fs::read` failed on a
    /// bundle-internal path.
    #[error("{op} {path}")]
    Io {
        op: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// `Command::output()` failed to launch a sub-process.
    #[error("running {display_name}")]
    RunCommand {
        display_name: String,
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
        kind: &'static str,
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
        snapshot_sha: String,
        current_sha: String,
    },
}
