//! Git repository information and operations.
//!
//! This module provides functionality for capturing git repository
//! state including commit hashes, branch info, and dirty status.

use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Command;

use thiserror::Error;

use crate::traits::GitProvider;
use crate::util::{CmdError, cmd_stdout};

/// Errors returned by this module's git-query helpers and by
/// [`GitProvider`] implementations.
#[derive(Debug, Error)]
pub enum GitError {
    /// A child `git` command failed to launch or ran to a non-zero
    /// exit, or produced non-UTF-8 output. See [`CmdError`] for the
    /// exact failure shape.
    #[error(transparent)]
    Cmd(#[from] CmdError),
    /// `git status --porcelain` or `git ls-files` launched but exited
    /// with a non-zero status, and the caller wants a specific label.
    #[error("{cmd} failed")]
    SubcommandFailed {
        /// Short label of the git subcommand (e.g. `"git status"`).
        cmd: String,
    },
    /// A cert/record profile requires valid git state, but the
    /// provider returned no or unknown SHA.
    #[error(
        "cert/record profile requires valid git state. Not in a git repository or HEAD is detached."
    )]
    StrictStateRequired,
    /// A cert/record profile requires a valid git branch, but the
    /// provider returned no or unknown branch.
    #[error(
        "cert/record profile requires valid git branch. Not in a git repository or HEAD is detached."
    )]
    StrictBranchRequired,
    /// A cert/record profile requires knowing whether the tree is
    /// dirty, but the provider failed to determine it.
    #[error(
        "cert/record profile requires git dirty status. Failed to determine working directory state."
    )]
    StrictDirtyRequired,
    /// `.git/shallow` exists — evidence generation needs full history.
    #[error(
        "Shallow clone detected. Evidence generation requires full repository history.\n\
         Run: git fetch --unshallow"
    )]
    ShallowClone,
    /// `git ls-files -z` returned bytes that are not valid UTF-8.
    /// Bundle JSON only carries UTF-8 path strings, so this is a hard
    /// failure rather than a lossy conversion.
    #[error("git ls-files returned non-UTF8 path (evidence requires UTF8)")]
    NonUtf8Path,
    /// Ad-hoc error — primarily for test doubles. Real implementations
    /// should prefer a more specific variant.
    #[error("{0}")]
    Other(String),
}

/// Git repository state snapshot.
///
/// Captures git state at a point in time to avoid race conditions
/// from querying git multiple times during evidence generation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitSnapshot {
    /// Current commit SHA
    pub sha: String,
    /// Current branch name
    pub branch: String,
    /// Whether the working directory is dirty
    pub dirty: bool,
}

impl GitSnapshot {
    /// Capture current git state using the real git provider.
    ///
    /// When `strict` is true (cert/record profiles), any failure to read git
    /// state is a hard error instead of falling back to "unknown". This
    /// ensures strict error handling in certification profiles.
    pub fn capture(strict: bool) -> Result<Self, GitError> {
        let provider = RealGitProvider;
        Self::capture_with(&provider, strict)
    }

    /// Capture git state using a custom provider.
    ///
    /// When `strict` is true, failures to obtain git SHA, branch, or dirty
    /// status are propagated as errors rather than silently defaulting.
    pub fn capture_with<G: GitProvider>(provider: &G, strict: bool) -> Result<Self, GitError> {
        let sha = provider.sha();
        let branch = provider.branch();
        let dirty = provider.is_dirty();

        if strict {
            let sha_val = sha.as_ref().map(|s| s.as_str()).unwrap_or("unknown");
            if sha.is_err() || sha_val == "unknown" {
                return Err(GitError::StrictStateRequired);
            }
            let branch_val = branch.as_ref().map(|s| s.as_str()).unwrap_or("unknown");
            if branch.is_err() || branch_val == "unknown" {
                return Err(GitError::StrictBranchRequired);
            }
            if dirty.is_err() {
                return Err(GitError::StrictDirtyRequired);
            }
        }

        Ok(Self {
            sha: sha.unwrap_or_else(|_| "unknown".to_string()),
            branch: branch.unwrap_or_else(|_| "unknown".to_string()),
            dirty: match dirty {
                Ok(d) => d,
                Err(_) => {
                    log::warn!(
                        "Could not determine git dirty status; defaulting to dirty. \
                         Safety-critical default: assume worst case when status is unknown."
                    );
                    true
                }
            },
        })
    }
}

/// Real git provider that executes git commands.
pub struct RealGitProvider;

impl GitProvider for RealGitProvider {
    fn sha(&self) -> Result<String, GitError> {
        git_sha()
    }

    fn branch(&self) -> Result<String, GitError> {
        git_branch()
    }

    fn is_dirty(&self) -> Result<bool, GitError> {
        git_dirty()
    }

    fn dirty_files(&self) -> Result<Vec<String>, GitError> {
        git_dirty_files()
    }
}

/// Get the current git commit SHA.
pub fn git_sha() -> Result<String, GitError> {
    Ok(cmd_stdout("git", &["rev-parse", "HEAD"])?
        .trim()
        .to_string())
}

/// Get the current git branch name.
pub fn git_branch() -> Result<String, GitError> {
    Ok(cmd_stdout("git", &["rev-parse", "--abbrev-ref", "HEAD"])?
        .trim()
        .to_string())
}

/// Safety-critical "is the working tree dirty?" check.
///
/// Returns `true` when `git status --porcelain` reports any output,
/// `false` when the output is empty, and — critically — `true` when
/// the check itself fails for any reason. Evidence bundles treat
/// unknown git state as dirty; this is the single source of truth for
/// that semantic.
///
/// Use this from CLI paths and from environment-fingerprint capture
/// where a `false` default would let a silently-failing git query
/// record a clean tree and mis-represent the build state.
pub fn is_dirty_or_unknown() -> bool {
    git_dirty().unwrap_or(true)
}

/// Refuse to run when the working copy is a shallow clone.
///
/// Evidence generation resolves git SHAs and needs complete history;
/// a shallow clone can silently produce bundles that cannot be
/// verified against a full repository later. Returns `Err` if
/// `.git/shallow` exists.
pub fn check_shallow_clone() -> Result<(), GitError> {
    if Path::new(".git/shallow").exists() {
        return Err(GitError::ShallowClone);
    }
    Ok(())
}

/// Check if the git working directory is dirty.
pub fn git_dirty() -> Result<bool, GitError> {
    let out = Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .map_err(|source| {
            GitError::Cmd(CmdError::Launch {
                prog: "git".to_string(),
                args: vec!["status".into(), "--porcelain".into()],
                source,
            })
        })?;
    if !out.status.success() {
        return Err(GitError::SubcommandFailed {
            cmd: "git status".to_string(),
        });
    }
    Ok(!out.stdout.is_empty())
}

/// Get list of dirty files (modified, untracked, etc.) for error reporting.
pub fn git_dirty_files() -> Result<Vec<String>, GitError> {
    let out = Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .map_err(|source| {
            GitError::Cmd(CmdError::Launch {
                prog: "git".to_string(),
                args: vec!["status".into(), "--porcelain".into()],
                source,
            })
        })?;
    if !out.status.success() {
        return Err(GitError::SubcommandFailed {
            cmd: "git status".to_string(),
        });
    }
    let output = String::from_utf8_lossy(&out.stdout);
    Ok(output
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| l.to_string())
        .collect())
}

/// Get list of files tracked by git matching the given prefixes.
///
/// Uses null-separated output for robustness with special characters.
/// Returns sorted list for determinism.
pub fn git_ls_files(prefixes: &[&str]) -> Result<Vec<String>, GitError> {
    let mut args: Vec<String> = vec!["ls-files".into(), "-z".into(), "--".into()];
    args.extend(prefixes.iter().map(|s| (*s).to_string()));

    let output = Command::new("git").args(&args).output().map_err(|source| {
        GitError::Cmd(CmdError::Launch {
            prog: "git".to_string(),
            args: args.clone(),
            source,
        })
    })?;
    if !output.status.success() {
        return Err(GitError::SubcommandFailed {
            cmd: "git ls-files".to_string(),
        });
    }

    // Parse null-separated output using bytes (safe for non-UTF8 paths).
    // Split by NUL byte, then validate UTF-8 for each segment.
    let mut files: Vec<String> = Vec::new();
    for segment in output.stdout.split(|b| *b == 0) {
        if segment.is_empty() {
            continue;
        }
        // Require valid UTF-8 paths for evidence integrity.
        let path = std::str::from_utf8(segment).map_err(|_| GitError::NonUtf8Path)?;
        files.push(path.to_string());
    }
    // Re-sort for determinism (git output is usually sorted, but be explicit).
    files.sort();
    Ok(files)
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

    #[test]
    fn test_git_snapshot_fields() {
        let snapshot = GitSnapshot {
            sha: "abc123".to_string(),
            branch: "main".to_string(),
            dirty: false,
        };
        assert_eq!(snapshot.sha, "abc123");
        assert_eq!(snapshot.branch, "main");
        assert!(!snapshot.dirty);
    }
}
