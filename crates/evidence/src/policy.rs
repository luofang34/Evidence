//! Verification policies.
//!
//! This module defines policies that control what aspects
//! of the build should be verified.

use serde::{Deserialize, Serialize};

/// A verification policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Policy {
    /// Whether to verify source hashes
    pub verify_sources: bool,
    /// Whether to verify output hashes
    pub verify_outputs: bool,
    /// Whether to verify environment
    pub verify_environment: bool,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            verify_sources: true,
            verify_outputs: true,
            verify_environment: false,
        }
    }
}
