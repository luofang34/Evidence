//! Build command tracing.
//!
//! This module provides functionality for tracing and recording
//! build commands and their outputs.

use serde::{Deserialize, Serialize};

/// A traced build command.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TracedCommand {
    /// The command that was executed
    pub command: String,
    /// Command arguments
    pub args: Vec<String>,
    /// Exit code
    pub exit_code: Option<i32>,
}

impl TracedCommand {
    /// Create a new traced command.
    pub fn new(command: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            args: Vec::new(),
            exit_code: None,
        }
    }
}
