//! `CommandRecord` — the per-invocation audit row that lands in `commands.json`.

use serde::{Deserialize, Serialize};

/// Record of a command execution for evidence.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CommandRecord {
    /// Command and arguments
    pub argv: Vec<String>,
    /// Working directory
    pub cwd: String,
    /// Exit code
    pub exit_code: i32,
    /// Path to stdout file (relative to bundle)
    pub stdout_path: Option<String>,
    /// Path to stderr file (relative to bundle)
    pub stderr_path: Option<String>,
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
    fn test_command_record_fields() {
        let rec = CommandRecord {
            argv: vec!["cargo".to_string(), "test".to_string()],
            cwd: "/project".to_string(),
            exit_code: 0,
            stdout_path: Some("stdout.txt".to_string()),
            stderr_path: Some("stderr.txt".to_string()),
        };
        assert_eq!(rec.exit_code, 0);
        assert_eq!(rec.argv.len(), 2);
    }
}
