//! Core abstraction traits for the evidence system.
//!
//! This module defines the core traits that enable testing and alternative
//! implementations of the evidence engine's dependencies (git, filesystem, etc.).

use anyhow::Result;
use std::path::{Path, PathBuf};

use crate::env::EnvFingerprint;

/// Output from running a command.
#[derive(Debug, Clone)]
pub struct CommandOutput {
    /// Exit code of the command
    pub exit_code: i32,
    /// Standard output bytes
    pub stdout: Vec<u8>,
    /// Standard error bytes
    pub stderr: Vec<u8>,
}

impl CommandOutput {
    /// Check if the command succeeded (exit code 0).
    pub fn success(&self) -> bool {
        self.exit_code == 0
    }

    /// Get stdout as a string, lossy conversion.
    pub fn stdout_str(&self) -> String {
        String::from_utf8_lossy(&self.stdout).to_string()
    }

    /// Get stderr as a string, lossy conversion.
    pub fn stderr_str(&self) -> String {
        String::from_utf8_lossy(&self.stderr).to_string()
    }
}

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

/// Trait for running shell commands.
pub trait CommandRunner {
    /// Run a command with the given arguments in the specified directory.
    fn run(&self, cmd: &[&str], cwd: &Path) -> Result<CommandOutput>;
}

/// Trait for filesystem operations.
pub trait FileSystem {
    /// Read the contents of a file.
    fn read(&self, path: &Path) -> Result<Vec<u8>>;

    /// Write content to a file.
    fn write(&self, path: &Path, content: &[u8]) -> Result<()>;

    /// Check if a path exists.
    fn exists(&self, path: &Path) -> bool;

    /// Walk a directory tree and return all file paths.
    fn walk(&self, root: &Path) -> Result<Vec<PathBuf>>;
}

/// Trait for detecting build environment.
pub trait EnvironmentDetector {
    /// Detect and capture the current build environment.
    fn detect(&self) -> Result<EnvFingerprint>;
}

