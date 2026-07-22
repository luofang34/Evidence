//! Inventory the workspace's compiled deliverables from
//! `cargo build --message-format=json` compiler-artifact messages.
//!
//! `outputs_hashes.json` records the SHA-256 of every deliverable an
//! in-scope crate's build produces (its `lib` / `bin` artifacts), so a
//! bundle attests the deliverables its build actually produced. Cargo
//! emits one `compiler-artifact` message per built target with the
//! absolute `filenames` it produced; only messages whose `package_id`
//! is an exact `cargo metadata` `workspace_members` entry are kept
//! (external deps, path deps outside the workspace, build scripts, and
//! proc-macros are excluded). Each artifact is keyed by its path
//! relative to cargo's `target_directory` — never an absolute path.
//!
//! The build follows the profile's flags plus the shared
//! [`ResolutionPolicy`]: cert/record profiles build `--release` (the
//! deliverable), and every profile carries the policy's resolution
//! flags — `--locked --offline` under `locked_offline`, none under the
//! development online opt-in. Output digests are host/build-specific —
//! they belong to the content-integrity channel, not the cross-host
//! reproducibility channel.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use thiserror::Error;

use crate::policy::{Profile, ResolutionPolicy};
use crate::util::{CmdError, cmd_stdout};

/// One inventoried deliverable: a canonical key relative to cargo's
/// `target_directory` and the absolute path to the produced file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputArtifact {
    /// Path relative to cargo's `target_directory`, forward-slash
    /// (e.g. `release/cargo-evidence`).
    pub key: String,
    /// Absolute path to the artifact file on disk.
    pub path: PathBuf,
}

/// Errors inventorying compiler artifacts.
#[derive(Debug, Error)]
pub enum ArtifactError {
    /// `cargo build`/`cargo metadata` failed to launch or exited non-zero.
    #[error("running `{cmd}` for output inventory")]
    Cargo {
        /// Which cargo invocation failed.
        cmd: &'static str,
        /// Underlying command error.
        #[source]
        source: CmdError,
    },
    /// The `--locked --offline` `cargo metadata` / `cargo build`
    /// invocation failed — the locked dependency graph could not be
    /// resolved from the local cargo cache (LLR-140).
    #[error(transparent)]
    LockedGraphUnavailable(#[from] crate::policy::LockedGraphError),
    /// The workspace metadata JSON was not the shape this module reads.
    #[error("parsing cargo metadata for output inventory")]
    ParseMetadata(#[source] serde_json::Error),
    /// A `cargo build --message-format=json` line was not valid JSON —
    /// surfaced rather than silently skipped so a format drift is loud.
    #[error("malformed cargo build message: {line}")]
    MalformedMessage {
        /// The offending line (truncated by the caller if needed).
        line: String,
        /// Underlying parse error.
        #[source]
        source: serde_json::Error,
    },
    /// A produced artifact is not under cargo's `target_directory`, so it
    /// has no canonical relative key — rejected rather than recorded
    /// under an absolute path.
    #[error("artifact '{path}' is not under the target directory '{target_dir}'")]
    ArtifactOutsideTargetDir {
        /// The offending absolute artifact path.
        path: String,
        /// The `target_directory` it was expected under.
        target_dir: String,
    },
}

/// Parse `cargo build --message-format=json` output into the workspace
/// deliverables. Keeps `compiler-artifact` messages whose `package_id`
/// is an exact `workspace_members` entry and whose target is a `lib` or
/// `bin`; every produced filename becomes an [`OutputArtifact`] keyed by
/// its path relative to `target_directory`. Fails closed on a malformed
/// message or an artifact outside `target_directory`. Sorted for
/// deterministic output.
pub fn parse_workspace_artifacts(
    build_json: &str,
    workspace_members: &BTreeSet<String>,
    target_directory: &Path,
) -> Result<Vec<OutputArtifact>, ArtifactError> {
    let mut out: Vec<OutputArtifact> = Vec::new();
    for line in build_json.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let msg: ArtifactMsg =
            serde_json::from_str(line).map_err(|source| ArtifactError::MalformedMessage {
                line: line.chars().take(200).collect(),
                source,
            })?;
        if msg.reason.as_deref() != Some("compiler-artifact") {
            continue;
        }
        let Some(pkg) = msg.package_id.as_deref() else {
            continue;
        };
        if !workspace_members.contains(pkg) {
            continue;
        }
        let kinds = msg
            .target
            .as_ref()
            .map(|t| t.kind.as_slice())
            .unwrap_or(&[]);
        if !kinds.iter().any(|k| k == "lib" || k == "bin") {
            continue;
        }
        for f in msg.filenames.into_iter().flatten() {
            let abs = PathBuf::from(&f);
            let rel = abs.strip_prefix(target_directory).map_err(|_| {
                ArtifactError::ArtifactOutsideTargetDir {
                    path: f.clone(),
                    target_dir: target_directory.display().to_string(),
                }
            })?;
            out.push(OutputArtifact {
                key: to_forward_slash(rel),
                path: abs,
            });
        }
    }
    out.sort_by(|a, b| a.key.cmp(&b.key));
    out.dedup();
    Ok(out)
}

/// Build the workspace with the profile's build flags and inventory its
/// deliverables. Blocks on I/O. `cargo metadata` supplies the exact
/// `workspace_members` set and the `target_directory`; cert/record
/// profiles build `--release` so the inventory attests the deliverable
/// rather than an implicit debug build. Both invocations carry the
/// given [`ResolutionPolicy`] flags (LLR-140), so the inventory attests
/// the pinned, offline-resolved dependency graph under `locked_offline`
/// and fails closed with [`ArtifactError::LockedGraphUnavailable`] when
/// that graph is unavailable from the local cache.
pub fn inventory_outputs_blocking(
    profile: Profile,
    policy: ResolutionPolicy,
) -> Result<Vec<OutputArtifact>, ArtifactError> {
    let mut meta_args = vec!["metadata", "--format-version", "1", "--no-deps"];
    meta_args.extend_from_slice(policy.cargo_args());
    let meta_json = cmd_stdout("cargo", &meta_args).map_err(|e| {
        match policy.offline_failure("cargo metadata", e) {
            Ok(g) => ArtifactError::LockedGraphUnavailable(g),
            Err(e) => ArtifactError::Cargo {
                cmd: "cargo metadata",
                source: e,
            },
        }
    })?;
    let meta: RawMeta = serde_json::from_str(&meta_json).map_err(ArtifactError::ParseMetadata)?;
    let members: BTreeSet<String> = meta.workspace_members.into_iter().collect();
    let target_directory = PathBuf::from(&meta.target_directory);

    let build_json = cmd_stdout("cargo", &build_args(profile, policy)).map_err(|e| match policy
        .offline_failure("cargo build", e)
    {
        Ok(g) => ArtifactError::LockedGraphUnavailable(g),
        Err(e) => ArtifactError::Cargo {
            cmd: "cargo build",
            source: e,
        },
    })?;
    parse_workspace_artifacts(&build_json, &members, &target_directory)
}

/// The `cargo build` args for a profile's output inventory. Cert/record
/// append `--release` (release deliverable); the resolution-policy
/// flags come from [`ResolutionPolicy::cargo_args`] — `--locked
/// --offline` under `locked_offline`, none under the development
/// online opt-in. Split out so the profile→flags mapping is
/// unit-testable without spawning cargo.
fn build_args(profile: Profile, policy: ResolutionPolicy) -> Vec<&'static str> {
    let mut args = vec!["build", "--workspace", "--message-format=json"];
    if matches!(profile, Profile::Cert | Profile::Record) {
        args.push("--release");
    }
    args.extend_from_slice(policy.cargo_args());
    args
}

fn to_forward_slash(p: &Path) -> String {
    p.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

#[derive(Debug, Deserialize)]
struct ArtifactMsg {
    reason: Option<String>,
    package_id: Option<String>,
    target: Option<ArtifactTarget>,
    #[serde(default)]
    filenames: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct ArtifactTarget {
    #[serde(default)]
    kind: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct RawMeta {
    workspace_members: Vec<String>,
    target_directory: String,
}

#[cfg(test)]
#[path = "cargo_artifacts/tests.rs"]
mod tests;
