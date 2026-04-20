//! Core abstraction traits for the evidence system.
//!
//! This module defines the core traits that enable testing and alternative
//! implementations of the evidence engine's dependencies (git, filesystem, etc.).

use crate::git::GitError;

/// Trait for git repository operations.
///
/// Implementers return [`GitError`] so callers can distinguish
/// "command failed" from "shallow clone detected" from test-mock
/// variants, without string-grepping. The `GitError::Other(String)`
/// variant is the escape hatch for ad-hoc error construction in
/// test doubles.
pub trait GitProvider {
    /// Get the current commit SHA.
    fn sha(&self) -> Result<String, GitError>;

    /// Get the current branch name.
    fn branch(&self) -> Result<String, GitError>;

    /// Check if the working directory is dirty.
    fn is_dirty(&self) -> Result<bool, GitError>;

    /// Get the list of dirty files (modified, untracked, etc.).
    fn dirty_files(&self) -> Result<Vec<String>, GitError>;
}
