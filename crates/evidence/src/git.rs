//! Git repository information.
//!
//! This module provides functionality for capturing git repository
//! state including commit hashes, branch info, and dirty status.

use serde::{Deserialize, Serialize};

/// Git repository state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitInfo {
    /// Current commit hash
    pub commit: Option<String>,
    /// Current branch name
    pub branch: Option<String>,
    /// Whether the working directory is dirty
    pub dirty: bool,
}

impl GitInfo {
    /// Capture git info from the current directory.
    pub fn capture() -> anyhow::Result<Option<Self>> {
        // TODO: Implement git info capture
        Ok(None)
    }
}
