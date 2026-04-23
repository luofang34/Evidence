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
//! All spawns are bounded by [`spawn_timeout`]. Default 10
//! minutes, tunable per-spawn via the `EVIDENCE_MCP_TIMEOUT_SECS`
//! env var (clamped to `[MIN_SPAWN_TIMEOUT_SECS,
//! MAX_SPAWN_TIMEOUT_SECS]`). A timeout yields
//! [`RunError::Timeout`] with no partial output — the tool
//! response surfaces the signal loud.

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
}

/// Errors raised by [`run_evidence`] when the subprocess
/// infrastructure itself fails — distinct from the subprocess
/// running and returning a non-zero exit code (which is a
/// [`Captured`] with `exit_code != 0`, not an error).
#[derive(Debug, thiserror::Error)]
pub(crate) enum RunError {
    /// `cargo` could not be resolved on `$PATH`. The `hint`
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

    /// Subprocess ran past the effective [`spawn_timeout`].
    /// Child has been killed by the time this error returns.
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

impl RunError {
    /// Return the stable `MCP_*` diagnostic code this variant
    /// maps to. Pinned on the enum (rather than matched in the
    /// response-building helper) so a new variant forces the
    /// code declaration at the type site — callers read the
    /// code directly, no external match arm to drift.
    ///
    /// Every returned value must appear in
    /// [`evidence_core::HAND_EMITTED_MCP_CODES`]; the
    /// `mcp_codes_audit::every_mcp_code_emitted_in_source`
    /// integration test closes the loop.
    pub(crate) fn code(&self) -> &'static str {
        match self {
            Self::BinaryNotFound { .. } => "MCP_CARGO_NOT_FOUND",
            Self::Spawn(_) => "MCP_SUBPROCESS_SPAWN_FAILED",
            Self::Timeout { .. } => "MCP_SUBPROCESS_TIMEOUT",
        }
    }
}

/// Default per-subprocess cap when `EVIDENCE_MCP_TIMEOUT_SECS`
/// is absent or empty. 10 minutes sized for `evidence_check`
/// source mode on a ~30-crate workspace running `cargo test
/// --workspace`.
pub(crate) const DEFAULT_SPAWN_TIMEOUT_SECS: u64 = 600;

/// Minimum accepted timeout. Below this, clamp up — the
/// shortest verbs (`rules`, `doctor` on a small workspace)
/// still need ~tens of seconds.
pub(crate) const MIN_SPAWN_TIMEOUT_SECS: u64 = 60;

/// Maximum accepted timeout. Caps the worst-case agent-driven
/// run so a host that sets `EVIDENCE_MCP_TIMEOUT_SECS=999999`
/// can't keep the subprocess alive indefinitely. 2 hours covers
/// the largest `cargo test --workspace + cargo-llvm-cov` run
/// this tool wraps.
pub(crate) const MAX_SPAWN_TIMEOUT_SECS: u64 = 7200;

/// Env var read by [`spawn_timeout`] on every call. Letting the
/// variable change between spawns means an operator can re-tune
/// the cap without restarting the MCP server.
pub(crate) const TIMEOUT_ENV_VAR: &str = "EVIDENCE_MCP_TIMEOUT_SECS";

/// Resolve the effective per-subprocess timeout. Thin shim
/// that reads [`TIMEOUT_ENV_VAR`] from the process and delegates
/// to [`resolve_timeout`]. Splitting the pure logic out makes it
/// unit-testable without racing against parallel tests that
/// also touch the environment.
pub(crate) fn spawn_timeout() -> Duration {
    resolve_timeout(std::env::var(TIMEOUT_ENV_VAR).ok().as_deref())
}

/// Apply the [`TIMEOUT_ENV_VAR`] decision table to a specific
/// raw value — absent, empty, parse-failure, or in/out of
/// `[MIN_SPAWN_TIMEOUT_SECS, MAX_SPAWN_TIMEOUT_SECS]` — and
/// return the resulting [`Duration`]. `None` and `Some("")`
/// both mean "env not provided".
///
/// - Unset / empty → [`DEFAULT_SPAWN_TIMEOUT_SECS`].
/// - Valid parse in-range → that value.
/// - Valid parse outside the bounds → clamp to the nearest
///   bound, `tracing::warn!` naming raw + clamped.
/// - Unparseable → `tracing::warn!` and fall back to default
///   (never panic — a typo in env shouldn't take the server
///   down).
///
/// Surface impact: on exceed, [`run_evidence`] emits
/// [`RunError::Timeout`] with `timeout_secs` reporting the
/// effective (post-clamp) value, not the raw env string.
pub(crate) fn resolve_timeout(raw: Option<&str>) -> Duration {
    let Some(raw) = raw.filter(|s| !s.is_empty()) else {
        return Duration::from_secs(DEFAULT_SPAWN_TIMEOUT_SECS);
    };
    let parsed: u64 = match raw.parse() {
        Ok(n) => n,
        Err(_) => {
            tracing::warn!(
                raw = raw,
                "{TIMEOUT_ENV_VAR} is not a non-negative integer; \
                 falling back to default {DEFAULT_SPAWN_TIMEOUT_SECS}s",
            );
            return Duration::from_secs(DEFAULT_SPAWN_TIMEOUT_SECS);
        }
    };
    if parsed < MIN_SPAWN_TIMEOUT_SECS {
        tracing::warn!(
            raw = parsed,
            clamped = MIN_SPAWN_TIMEOUT_SECS,
            "{TIMEOUT_ENV_VAR} below minimum; clamping up to {MIN_SPAWN_TIMEOUT_SECS}s",
        );
        return Duration::from_secs(MIN_SPAWN_TIMEOUT_SECS);
    }
    if parsed > MAX_SPAWN_TIMEOUT_SECS {
        tracing::warn!(
            raw = parsed,
            clamped = MAX_SPAWN_TIMEOUT_SECS,
            "{TIMEOUT_ENV_VAR} above maximum; clamping down to {MAX_SPAWN_TIMEOUT_SECS}s",
        );
        return Duration::from_secs(MAX_SPAWN_TIMEOUT_SECS);
    }
    Duration::from_secs(parsed)
}

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
        })
    };

    let effective = spawn_timeout();
    match tokio::time::timeout(effective, spawn_future).await {
        Ok(result) => result,
        Err(_elapsed) => Err(RunError::Timeout {
            timeout_secs: effective.as_secs(),
        }),
    }
}

/// Parse a JSONL byte stream into (terminal_code, all_events,
/// per-code summary counts). Trailing blank lines are ignored.
///
/// Empty stdout yields terminal [`MCP_NO_OUTPUT`]. Each non-
/// empty line parses as a `serde_json::Value`; an un-parseable
/// line is replaced with a synthesized [`MCP_MALFORMED_JSONL`]
/// marker and the terminal stays `MCP_MALFORMED_JSONL` for the
/// rest of the stream so callers get a well-formed response.
///
/// [`MCP_NO_OUTPUT`]: MCP_NO_OUTPUT
/// [`MCP_MALFORMED_JSONL`]: MCP_MALFORMED_JSONL
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
                let marker = serde_json::json!({
                    "code": MCP_MALFORMED_JSONL,
                    "severity": "error",
                    "message": format!("evidence-mcp could not parse line as JSON: {}", line),
                });
                *summary.entry(MCP_MALFORMED_JSONL.to_string()).or_insert(0) += 1;
                terminal = MCP_MALFORMED_JSONL.to_string();
                events.push(marker);
            }
        }
    }

    if events.is_empty() {
        terminal = MCP_NO_OUTPUT.to_string();
    }

    (terminal, events, summary)
}

/// Synthesized terminal for a subprocess that produced zero
/// stdout lines. Declared here (source-of-truth) and audited
/// against `evidence_core::HAND_EMITTED_MCP_CODES` by the
/// `mcp_codes_audit::every_mcp_code_emitted_in_source` test.
pub(crate) const MCP_NO_OUTPUT: &str = "MCP_NO_OUTPUT";

/// Synthesized marker for a stdout line that is not valid JSON.
/// Declared here (source-of-truth); audited alongside
/// [`MCP_NO_OUTPUT`].
pub(crate) const MCP_MALFORMED_JSONL: &str = "MCP_MALFORMED_JSONL";

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
    fn parse_jsonl_empty_stream_is_mcp_no_output() {
        let (terminal, events, summary) = parse_jsonl(b"");
        assert_eq!(terminal, "MCP_NO_OUTPUT");
        assert!(events.is_empty());
        assert!(summary.is_empty());
    }

    #[test]
    fn parse_jsonl_malformed_line_surfaces_mcp_marker() {
        let stream = b"{\"code\":\"REQ_PASS\"}\nnot valid json\n";
        let (terminal, events, summary) = parse_jsonl(stream);
        assert_eq!(terminal, "MCP_MALFORMED_JSONL");
        assert_eq!(events.len(), 2);
        assert_eq!(summary.get("MCP_MALFORMED_JSONL").copied(), Some(1));
    }

    #[test]
    fn run_error_code_is_self_describing() {
        use std::io::{Error, ErrorKind};

        let cases: [(RunError, &str); 3] = [
            (
                RunError::BinaryNotFound {
                    hint: "stub".into(),
                },
                "MCP_CARGO_NOT_FOUND",
            ),
            (
                RunError::Spawn(Error::new(ErrorKind::PermissionDenied, "denied")),
                "MCP_SUBPROCESS_SPAWN_FAILED",
            ),
            (
                RunError::Timeout { timeout_secs: 60 },
                "MCP_SUBPROCESS_TIMEOUT",
            ),
        ];
        for (err, expected) in &cases {
            assert_eq!(
                err.code(),
                *expected,
                "RunError variant {:?} advertised wrong code",
                err
            );
        }

        // Sanity-check the declared codes exist in the
        // evidence-core hand-emitted MCP registry. A typo or a
        // rename of `HAND_EMITTED_MCP_CODES` surfaces here
        // rather than from a downstream bijection run.
        let registry: std::collections::BTreeSet<&str> = evidence_core::HAND_EMITTED_MCP_CODES
            .iter()
            .copied()
            .collect();
        for (_err, expected) in &cases {
            assert!(
                registry.contains(*expected),
                "RunError declares code {:?} which is not in HAND_EMITTED_MCP_CODES",
                expected
            );
        }
    }

    #[test]
    fn resolve_timeout_defaults_when_env_unset() {
        assert_eq!(
            resolve_timeout(None),
            Duration::from_secs(DEFAULT_SPAWN_TIMEOUT_SECS)
        );
    }

    #[test]
    fn resolve_timeout_defaults_when_env_empty() {
        // Empty string is semantically "unset" — some launchers
        // (Nix sandbox, CI harnesses) export empty variables by
        // default; falling to the default here matches the
        // workspace convention recorded in project memory.
        assert_eq!(
            resolve_timeout(Some("")),
            Duration::from_secs(DEFAULT_SPAWN_TIMEOUT_SECS)
        );
    }

    #[test]
    fn resolve_timeout_parses_valid_value() {
        assert_eq!(resolve_timeout(Some("900")), Duration::from_secs(900));
        assert_eq!(
            resolve_timeout(Some(&MIN_SPAWN_TIMEOUT_SECS.to_string())),
            Duration::from_secs(MIN_SPAWN_TIMEOUT_SECS)
        );
        assert_eq!(
            resolve_timeout(Some(&MAX_SPAWN_TIMEOUT_SECS.to_string())),
            Duration::from_secs(MAX_SPAWN_TIMEOUT_SECS)
        );
    }

    #[test]
    fn resolve_timeout_clamps_below_minimum() {
        assert_eq!(
            resolve_timeout(Some("1")),
            Duration::from_secs(MIN_SPAWN_TIMEOUT_SECS)
        );
        assert_eq!(
            resolve_timeout(Some("0")),
            Duration::from_secs(MIN_SPAWN_TIMEOUT_SECS)
        );
    }

    #[test]
    fn resolve_timeout_clamps_above_maximum() {
        assert_eq!(
            resolve_timeout(Some("999999")),
            Duration::from_secs(MAX_SPAWN_TIMEOUT_SECS)
        );
    }

    #[test]
    fn resolve_timeout_falls_back_on_malformed_env() {
        // Unparseable → default, not panic.
        assert_eq!(
            resolve_timeout(Some("not-a-number")),
            Duration::from_secs(DEFAULT_SPAWN_TIMEOUT_SECS)
        );
        // Negative integers fail u64 parse — same fallback path.
        assert_eq!(
            resolve_timeout(Some("-30")),
            Duration::from_secs(DEFAULT_SPAWN_TIMEOUT_SECS)
        );
    }
}
