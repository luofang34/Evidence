//! Git repository information and operations.
//!
//! This module provides functionality for capturing git repository
//! state including commit hashes, branch info, and dirty status.

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::process::Command;

use crate::traits::GitProvider;
use crate::util::cmd_stdout;

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
    pub fn capture(strict: bool) -> Result<Self> {
        let provider = RealGitProvider;
        Self::capture_with(&provider, strict)
    }

    /// Capture git state using a custom provider.
    ///
    /// When `strict` is true, failures to obtain git SHA, branch, or dirty
    /// status are propagated as errors rather than silently defaulting.
    pub fn capture_with<G: GitProvider>(provider: &G, strict: bool) -> Result<Self> {
        let sha = provider.sha();
        let branch = provider.branch();
        let dirty = provider.is_dirty();

        if strict {
            let sha_val = sha.as_ref().map(|s| s.as_str()).unwrap_or("unknown");
            if sha.is_err() || sha_val == "unknown" {
                bail!(
                    "cert/record profile requires valid git state. \
                     Not in a git repository or HEAD is detached."
                );
            }
            let branch_val = branch.as_ref().map(|s| s.as_str()).unwrap_or("unknown");
            if branch.is_err() || branch_val == "unknown" {
                bail!(
                    "cert/record profile requires valid git branch. \
                     Not in a git repository or HEAD is detached."
                );
            }
            if dirty.is_err() {
                bail!(
                    "cert/record profile requires git dirty status. \
                     Failed to determine working directory state."
                );
            }
        }

        Ok(Self {
            sha: sha.unwrap_or_else(|_| "unknown".to_string()),
            branch: branch.unwrap_or_else(|_| "unknown".to_string()),
            dirty: match dirty {
                Ok(d) => d,
                Err(_) => {
                    tracing::warn!(
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
    fn sha(&self) -> Result<String> {
        git_sha()
    }

    fn branch(&self) -> Result<String> {
        git_branch()
    }

    fn is_dirty(&self) -> Result<bool> {
        git_dirty()
    }

    fn dirty_files(&self) -> Result<Vec<String>> {
        git_dirty_files()
    }
}

/// Get the current git commit SHA.
pub fn git_sha() -> Result<String> {
    Ok(cmd_stdout("git", &["rev-parse", "HEAD"])?
        .trim()
        .to_string())
}

/// Get the current git branch name.
pub fn git_branch() -> Result<String> {
    Ok(cmd_stdout("git", &["rev-parse", "--abbrev-ref", "HEAD"])?
        .trim()
        .to_string())
}

/// Check if the git working directory is dirty.
pub fn git_dirty() -> Result<bool> {
    let out = Command::new("git")
        .args(["status", "--porcelain"])
        .output()?;
    if !out.status.success() {
        bail!("git status failed");
    }
    Ok(!out.stdout.is_empty())
}

/// Get list of dirty files (modified, untracked, etc.) for error reporting.
pub fn git_dirty_files() -> Result<Vec<String>> {
    let out = Command::new("git")
        .args(["status", "--porcelain"])
        .output()?;
    if !out.status.success() {
        bail!("git status failed");
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
pub fn git_ls_files(prefixes: &[&str]) -> Result<Vec<String>> {
    let mut args = vec!["ls-files", "-z", "--"];
    args.extend(prefixes.iter().copied());

    let output = Command::new("git").args(&args).output()?;
    if !output.status.success() {
        bail!("git ls-files failed");
    }

    // Parse null-separated output using bytes (safe for non-UTF8 paths)
    // Split by NUL byte, then validate UTF-8 for each segment
    let mut files: Vec<String> = Vec::new();
    for segment in output.stdout.split(|b| *b == 0) {
        if segment.is_empty() {
            continue;
        }
        // Require valid UTF-8 paths for evidence integrity
        let path = std::str::from_utf8(segment).map_err(|_| {
            anyhow::anyhow!("git ls-files returned non-UTF8 path (evidence requires UTF8)")
        })?;
        files.push(path.to_string());
    }
    // Re-sort for determinism (git output is usually sorted, but be explicit)
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
