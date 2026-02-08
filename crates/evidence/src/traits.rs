//! Core abstraction traits for the evidence system.
//!
//! This module defines the core traits that enable testing and alternative
//! implementations of the evidence engine's dependencies (git, filesystem, etc.).

use anyhow::Result;

/// Trait for git repository operations.
pub trait GitProvider {
    /// Get the current commit SHA.
    fn sha(&self) -> Result<String>;

    /// Get the current branch name.
    fn branch(&self) -> Result<String>;

    /// Check if the working directory is dirty.
    fn is_dirty(&self) -> Result<bool>;

    /// Get the list of dirty files (modified, untracked, etc.).
    fn dirty_files(&self) -> Result<Vec<String>>;
}
