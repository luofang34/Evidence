//! Subprocess plumbing for `cargo evidence <verb>` spawns.
//!
//! Every MCP tool call on [`crate::Server`] resolves through
//! [`run_evidence`]. Binary resolution checks `$CARGO` first
//! (set when the MCP server itself runs under `cargo run`), then
//! falls back to invoking `cargo evidence <verb>` via `cargo` on
//! `$PATH`. Two env vars are force-set on the child — every
//! spawn runs with `CARGO_TERM_COLOR=never` and `NO_COLOR=1` so
//! ANSI escapes cannot corrupt the stdout JSONL stream the tool
//! layer parses.
//!
//! All spawns are bounded by [`SPAWN_TIMEOUT`]. `evidence_check`
//! source mode runs `cargo test --workspace` and can legitimately
//! take several minutes on large workspaces; 10 minutes is the
//! pragmatic cap. A timeout yields [`RunError::Timeout`] with no
//! partial output — the tool response surfaces the signal loud.

use std::path::Path;
use std::time::Duration;

use tokio::process::Command;

/// Captured output from a single `cargo evidence` subprocess.
///
/// Held by value in-memory; for the verbs this crate wraps
/// (`check`, `rules`, `doctor`) stdout is bounded by the number
/// of emitted diagnostics (hundreds at most) and stays small.
#[derive(Debug)]
pub(crate) struct Captured {
    /// Process exit code. `-1` sentinel if the child was killed
    /// by signal without producing an exit status.
    pub exit_code: i32,
    /// Raw stdout bytes. Callers parse as UTF-8 + JSONL.
    pub stdout: Vec<u8>,
    /// Raw stderr bytes. Informational for the caller; the CLI's
    /// stdout-strict contract keeps structured output on stdout.
    #[allow(dead_code)]
    pub stderr: Vec<u8>,
}

/// Errors raised by [`run_evidence`] when the subprocess
/// infrastructure itself fails — distinct from the subprocess
/// running and returning a non-zero exit code (which is a
/// [`Captured`] with `exit_code != 0`, not an error).
#[derive(Debug, thiserror::Error)]
pub(crate) enum RunError {
    /// `cargo` could not be resolved on `$PATH`. The `where()`
    /// field holds the lookup hint shown to the agent.
    #[error(
        "cargo not found on PATH; install a Rust toolchain (https://rustup.rs) \
         or ensure `~/.cargo/bin` is on PATH. Hint: {hint}"
    )]
    BinaryNotFound {
        /// Platform-specific diagnostic hint (e.g., current
        /// `$PATH` or the result of `which cargo`).
        hint: String,
    },

    /// The spawn syscall itself failed — permission denied,
    /// out of file descriptors, etc.
    #[error("spawning cargo evidence: {0}")]
    Spawn(#[source] std::io::Error),

    /// Subprocess ran past [`SPAWN_TIMEOUT`]. Child has been
    /// killed by the time this error returns.
    #[error(
        "cargo evidence exceeded the {timeout_secs}s timeout; partial \
         output discarded. If this was `evidence_check` on a large \
         workspace, consider running the CLI directly."
    )]
    Timeout {
        /// Configured timeout in seconds, for the error message.
        timeout_secs: u64,
    },
}

/// 10-minute cap on every subprocess. Sized for
/// `evidence_check` source mode on a ~30-crate workspace with
/// `cargo test --workspace` under it.
pub(crate) const SPAWN_TIMEOUT: Duration = Duration::from_secs(600);

/// Spawn `cargo evidence <args>` in `cwd` and capture stdout +
/// exit code.
///
/// `args` should include the verb and its flags but NOT the
/// leading `"evidence"` — this helper prepends it. Example:
/// `run_evidence(&["rules", "--json"], cwd)`.
pub(crate) async fn run_evidence(args: &[&str], cwd: &Path) -> Result<Captured, RunError> {
    let mut cmd = Command::new("cargo");
    cmd.arg("evidence");
    cmd.args(args);
    cmd.current_dir(cwd);
    cmd.env("CARGO_TERM_COLOR", "never");
    cmd.env("NO_COLOR", "1");
    cmd.kill_on_drop(true);

    let spawn_future = async {
        let output = cmd.output().await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                RunError::BinaryNotFound {
                    hint: format!(
                        "spawn failed: {}. Check `which cargo` and $PATH in the environment \
                         that launched this MCP server.",
                        e
                    ),
                }
            } else {
                RunError::Spawn(e)
            }
        })?;
        Ok::<Captured, RunError>(Captured {
            exit_code: output.status.code().unwrap_or(-1),
            stdout: output.stdout,
            stderr: output.stderr,
        })
    };

    match tokio::time::timeout(SPAWN_TIMEOUT, spawn_future).await {
        Ok(result) => result,
        Err(_elapsed) => Err(RunError::Timeout {
            timeout_secs: SPAWN_TIMEOUT.as_secs(),
        }),
    }
}

/// Parse a JSONL byte stream into (terminal_code, all_events,
/// per-code summary counts). Trailing blank lines are ignored.
///
/// Returns `None` if stdout is empty. Each non-empty line is
/// parsed as a `serde_json::Value`; parse failures collapse the
/// whole response to a synthesized `"MALFORMED_JSONL"` entry so
/// callers always get a well-formed response.
pub(crate) fn parse_jsonl(
    stdout: &[u8],
) -> (
    String,
    Vec<serde_json::Value>,
    std::collections::BTreeMap<String, u32>,
) {
    let text = String::from_utf8_lossy(stdout);
    let mut events: Vec<serde_json::Value> = Vec::new();
    let mut summary: std::collections::BTreeMap<String, u32> = std::collections::BTreeMap::new();
    let mut terminal = String::new();

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str::<serde_json::Value>(line) {
            Ok(value) => {
                if let Some(code) = value.get("code").and_then(|c| c.as_str()) {
                    *summary.entry(code.to_string()).or_insert(0) += 1;
                    terminal = code.to_string();
                }
                events.push(value);
            }
            Err(_) => {
                // Synthesize a malformed-line marker; keep parsing
                // to surface as much of the stream as we can.
                let marker = serde_json::json!({
                    "code": "MALFORMED_JSONL",
                    "severity": "error",
                    "message": format!("mcp-evidence could not parse line as JSON: {}", line),
                });
                *summary.entry("MALFORMED_JSONL".to_string()).or_insert(0) += 1;
                terminal = "MALFORMED_JSONL".to_string();
                events.push(marker);
            }
        }
    }

    if events.is_empty() {
        terminal = "NO_OUTPUT".to_string();
    }

    (terminal, events, summary)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "test-only")]
mod tests {
    use super::*;

    #[test]
    fn parse_jsonl_extracts_terminal_and_counts() {
        let stream = br#"{"code":"REQ_PASS","severity":"info","message":"ok"}
{"code":"REQ_PASS","severity":"info","message":"ok2"}
{"code":"REQ_GAP","severity":"error","message":"miss"}
{"code":"VERIFY_FAIL","severity":"error","message":"1 gap"}
"#;
        let (terminal, events, summary) = parse_jsonl(stream);
        assert_eq!(terminal, "VERIFY_FAIL");
        assert_eq!(events.len(), 4);
        assert_eq!(summary.get("REQ_PASS").copied(), Some(2));
        assert_eq!(summary.get("REQ_GAP").copied(), Some(1));
        assert_eq!(summary.get("VERIFY_FAIL").copied(), Some(1));
    }

    #[test]
    fn parse_jsonl_empty_stream_is_no_output() {
        let (terminal, events, summary) = parse_jsonl(b"");
        assert_eq!(terminal, "NO_OUTPUT");
        assert!(events.is_empty());
        assert!(summary.is_empty());
    }

    #[test]
    fn parse_jsonl_malformed_line_surfaces_marker() {
        let stream = b"{\"code\":\"REQ_PASS\"}\nnot valid json\n";
        let (terminal, events, summary) = parse_jsonl(stream);
        assert_eq!(terminal, "MALFORMED_JSONL");
        assert_eq!(events.len(), 2);
        assert_eq!(summary.get("MALFORMED_JSONL").copied(), Some(1));
    }
}
