//! Inventory the workspace's compiled deliverables from
//! `cargo build --message-format=json` compiler-artifact messages.
//!
//! `outputs_hashes.json` records the SHA-256 of every build output an
//! in-scope crate produces (its `lib` / `bin` artifacts), so a bundle
//! can attest what the recorded recipe actually built. Cargo emits one
//! `compiler-artifact` message per built target with the absolute
//! `filenames` it produced; workspace members carry a
//! `path+file://<workspace_root>/…` package id, which distinguishes
//! them from external dependencies. Build-script and proc-macro targets
//! are not deliverables and are excluded.
//!
//! Output digests are inherently host/build-specific — they belong to
//! the content-integrity channel, not the cross-host recipe channel.

use std::path::{Path, PathBuf};

use serde::Deserialize;
use thiserror::Error;

use crate::util::{CmdError, cmd_stdout};

/// One inventoried deliverable: a canonical workspace-relative key and
/// the absolute path to the produced file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputArtifact {
    /// Workspace-relative, forward-slash path (e.g. `target/release/cargo-evidence`).
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
    /// The workspace metadata JSON was not the shape this module reads.
    #[error("parsing cargo metadata for output inventory")]
    ParseMetadata(#[source] serde_json::Error),
}

/// Parse `cargo build --message-format=json` output into the workspace
/// deliverables under `workspace_root`. Only `compiler-artifact`
/// messages for workspace members (package id under `workspace_root`)
/// with a `lib` or `bin` target are kept; every produced filename
/// becomes an [`OutputArtifact`] keyed by its workspace-relative path.
/// Determinism: results are sorted by key.
pub fn parse_workspace_artifacts(build_json: &str, workspace_root: &Path) -> Vec<OutputArtifact> {
    let root_prefix = format!("path+file://{}", workspace_root.display());
    let mut out: Vec<OutputArtifact> = Vec::new();
    for line in build_json.lines() {
        let msg: ArtifactMsg = match serde_json::from_str(line) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if msg.reason.as_deref() != Some("compiler-artifact") {
            continue;
        }
        let Some(pkg) = msg.package_id.as_deref() else {
            continue;
        };
        if !pkg.starts_with(&root_prefix) {
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
            let key = abs
                .strip_prefix(workspace_root)
                .ok()
                .map(to_forward_slash)
                .unwrap_or_else(|| f.clone());
            out.push(OutputArtifact { key, path: abs });
        }
    }
    out.sort_by(|a, b| a.key.cmp(&b.key));
    out.dedup();
    out
}

/// Build the workspace and inventory its deliverables. Blocks on I/O.
/// Runs `cargo build --workspace --message-format=json` (fast when the
/// test phase already compiled the workspace) and resolves the
/// workspace root from `cargo metadata`.
pub fn inventory_outputs_blocking() -> Result<Vec<OutputArtifact>, ArtifactError> {
    let meta = cmd_stdout("cargo", &["metadata", "--format-version", "1", "--no-deps"]).map_err(
        |source| ArtifactError::Cargo {
            cmd: "cargo metadata",
            source,
        },
    )?;
    let root: RawRoot = serde_json::from_str(&meta).map_err(ArtifactError::ParseMetadata)?;
    let build = cmd_stdout("cargo", &["build", "--workspace", "--message-format=json"]).map_err(
        |source| ArtifactError::Cargo {
            cmd: "cargo build",
            source,
        },
    )?;
    Ok(parse_workspace_artifacts(
        &build,
        Path::new(&root.workspace_root),
    ))
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
struct RawRoot {
    workspace_root: String,
}

#[cfg(test)]
#[path = "cargo_artifacts/tests.rs"]
mod tests;
