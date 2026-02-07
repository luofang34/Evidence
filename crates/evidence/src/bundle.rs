//! Evidence bundle creation and management.
//!
//! This module handles the creation and manipulation of evidence bundles
//! that capture build artifacts, hashes, and metadata.

use serde::{Deserialize, Serialize};

/// Represents an evidence bundle containing build artifacts and metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bundle {
    /// Bundle format version
    pub version: String,
}

impl Bundle {
    /// Create a new empty bundle.
    pub fn new() -> Self {
        Self {
            version: "0.1.0".to_string(),
        }
    }
}

impl Default for Bundle {
    fn default() -> Self {
        Self::new()
    }
}
