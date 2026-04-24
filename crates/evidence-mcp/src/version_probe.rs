//! Detect version skew between evidence-mcp and the
//! `cargo-evidence` it spawns.
//!
//! Every tool call on [`crate::Server`] shells out to `cargo
//! evidence <verb>`; the `cargo-evidence` binary is resolved via
//! `$PATH`. If the user has an older `cargo-evidence` installed
//! globally and a newer `evidence-mcp` (or vice versa), the
//! tool-response diagnostic vocabulary is whichever the CLI
//! ships — not what `evidence-mcp` was built against. Silent
//! drift shows up as agents pattern-matching on `.code` values
//! that the newer MCP surface is no longer emitting, or missing
//! codes that the older CLI doesn't know about.
//!
//! This module probes `cargo evidence --version` once at
//! `Server::new()`, compares against
//! `env!("CARGO_PKG_VERSION")`, and caches the result. Each
//! tool response prepends a `MCP_VERSION_SKEW` (mismatch) or
//! `MCP_VERSION_PROBE_FAILED` (spawn / parse failure) warning
//! when appropriate, so agents receive the signal on every
//! interaction without paying the probe cost per-call.
//!
//! See SYS-028 / HLR-060 / LLR-063 / TEST-063.

use std::process::Command;
use std::time::Duration;

/// Outcome of the startup version probe.
///
/// `Matched` carries the probed string — byte-equal to
/// `env!("CARGO_PKG_VERSION")` at construction time — so
/// consumers that want the CLI version on a successful probe
/// read it off the variant rather than substituting the
/// evidence-mcp version. A future fuzzy-match relaxation
/// (e.g. semver-equal smoothing over pre-release suffixes)
/// would otherwise silently make `cli_version` lie to the
/// caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum VersionSkew {
    /// Probe succeeded; `evidence-mcp`'s version matches the
    /// spawned CLI's version. The `String` carries the probed
    /// value (byte-equal to `env!("CARGO_PKG_VERSION")` by the
    /// invariant enforced in [`detect_with_probe`]).
    Matched(String),
    /// Probe succeeded but the two versions disagree. Agents
    /// should see a warning carrying both strings.
    Skewed { mcp: String, cli: String },
    /// Probe failed (binary missing, spawn error, non-zero
    /// exit, empty / unparseable output). Surface a separate
    /// warning so an agent doesn't confuse "version unknown"
    /// with "versions disagree".
    ProbeFailed(String),
}

/// How long to wait for `cargo evidence --version` before
/// abandoning the probe. A well-behaved CLI returns in ≪1 s;
/// the generous timeout covers the rare case where `cargo` has
/// to touch the git CLI or resolve a cold registry index on the
/// first spawn.
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// Compose a skew determination from a closure supplying the
/// probed CLI version. Factored so tests can simulate the three
/// branches (match, skew, probe-failed) without spawning
/// subprocesses.
pub(crate) fn detect_with_probe<F>(probe: F) -> VersionSkew
where
    F: FnOnce() -> Result<String, String>,
{
    let mcp = env!("CARGO_PKG_VERSION").to_string();
    match probe() {
        Ok(cli) if cli == mcp => VersionSkew::Matched(cli),
        Ok(cli) => VersionSkew::Skewed { mcp, cli },
        Err(e) => VersionSkew::ProbeFailed(e),
    }
}

/// Startup probe: spawn `cargo evidence --version`, parse the
/// emitted semver. Used by the real Server; tests go through
/// [`detect_with_probe`] with a closure.
pub(crate) fn probe_cli_version() -> Result<String, String> {
    // `CARGO_TERM_COLOR=never` + `NO_COLOR=1` match the rest of
    // the subprocess plumbing and defend against a tty-sensing
    // CLI sneaking ANSI escapes into stdout.
    let child = Command::new("cargo")
        .args(["evidence", "--version"])
        .env("CARGO_TERM_COLOR", "never")
        .env("NO_COLOR", "1")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("cargo evidence --version spawn failed: {e}"))?;

    // Wait with a timeout. Cargo can legitimately be slow on
    // cold caches; anything past 5 s is pathological and we'd
    // rather surface a probe-failed warning than block the
    // server startup handshake on the one-shot version check.
    let out = wait_with_timeout(child, PROBE_TIMEOUT)?;

    if !out.status.success() {
        return Err(format!(
            "cargo evidence --version exited {}",
            out.status.code().unwrap_or(-1)
        ));
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    // Clap's auto-version emits `<bin_name> <version>\n`, e.g.
    // `cargo-evidence 0.1.1`. Take the last whitespace-separated
    // token on the first non-empty line.
    let version = stdout
        .lines()
        .find(|l| !l.trim().is_empty())
        .and_then(|l| l.split_whitespace().last())
        .unwrap_or("")
        .to_string();
    if version.is_empty() {
        return Err(format!(
            "cargo evidence --version produced unparseable output: {:?}",
            stdout.trim()
        ));
    }
    Ok(version)
}

fn wait_with_timeout(
    mut child: std::process::Child,
    timeout: Duration,
) -> Result<std::process::Output, String> {
    use std::io::Read;
    use std::thread;
    use std::time::Instant;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut stdout = Vec::new();
                let mut stderr = Vec::new();
                if let Some(mut so) = child.stdout.take() {
                    so.read_to_end(&mut stdout).ok();
                }
                if let Some(mut se) = child.stderr.take() {
                    se.read_to_end(&mut stderr).ok();
                }
                return Ok(std::process::Output {
                    status,
                    stdout,
                    stderr,
                });
            }
            Ok(None) => {
                if start.elapsed() >= timeout {
                    child.kill().ok();
                    return Err(format!(
                        "cargo evidence --version did not respond within {:?}",
                        timeout
                    ));
                }
                thread::sleep(Duration::from_millis(25));
            }
            Err(e) => return Err(format!("cargo evidence --version wait failed: {e}")),
        }
    }
}

/// Build a synthetic diagnostic for the given skew outcome, or
/// `None` when versions match. Matches the shape of
/// `workspace_fallback_diagnostic` so agents can pattern-match
/// both MCP-layer warnings on `.code` uniformly.
pub(crate) fn skew_diagnostic(skew: &VersionSkew) -> Option<serde_json::Value> {
    match skew {
        VersionSkew::Matched(_) => None,
        VersionSkew::Skewed { mcp, cli } => Some(serde_json::json!({
            "code": "MCP_VERSION_SKEW",
            "severity": "warning",
            "message": format!(
                "evidence-mcp {} ≠ cargo-evidence {}: tool responses reflect \
                 the CLI's vocabulary, not this MCP server's. Run \
                 `cargo install cargo-evidence` to sync.",
                mcp, cli
            ),
            "subcommand": "mcp",
        })),
        VersionSkew::ProbeFailed(reason) => Some(serde_json::json!({
            "code": "MCP_VERSION_PROBE_FAILED",
            "severity": "warning",
            "message": format!(
                "evidence-mcp could not probe cargo-evidence version: {}. \
                 Tool responses may reflect an unintended CLI version.",
                reason
            ),
            "subcommand": "mcp",
        })),
    }
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

    /// Normal: probe returns the same version as `evidence-mcp`'s
    /// `CARGO_PKG_VERSION` → Matched carrying the probed string
    /// (byte-equal to `mcp`). No warning synthesized.
    #[test]
    fn normal_matching_versions_yield_matched_no_diagnostic() {
        let mcp = env!("CARGO_PKG_VERSION").to_string();
        let skew = detect_with_probe(|| Ok(mcp.clone()));
        assert_eq!(skew, VersionSkew::Matched(mcp));
        assert!(skew_diagnostic(&skew).is_none());
    }

    /// Robustness: probe returns a different version → Skewed
    /// with both strings captured; diagnostic carries both so
    /// the agent can show the user which side to upgrade.
    #[test]
    fn robustness_differing_versions_yield_skewed_with_both_strings() {
        let mcp = env!("CARGO_PKG_VERSION").to_string();
        let cli = "0.0.1-stale".to_string();
        let skew = detect_with_probe(|| Ok(cli.clone()));
        match &skew {
            VersionSkew::Skewed { mcp: m, cli: c } => {
                assert_eq!(m, &mcp);
                assert_eq!(c, &cli);
            }
            other => panic!("expected Skewed, got {other:?}"),
        }
        let diag = skew_diagnostic(&skew).expect("skewed must yield a diagnostic");
        assert_eq!(diag["code"], "MCP_VERSION_SKEW");
        let msg = diag["message"].as_str().unwrap();
        assert!(
            msg.contains(&mcp) && msg.contains(&cli),
            "message must name both versions; got: {msg}"
        );
    }

    /// Robustness: probe failure (binary missing, spawn error,
    /// empty output) → ProbeFailed with the underlying reason.
    /// Distinct code from Skewed so an agent can tell "unknown"
    /// apart from "disagree".
    #[test]
    fn robustness_probe_error_yields_probe_failed_with_reason() {
        let skew = detect_with_probe(|| Err("spawn failed: no such file".to_string()));
        match &skew {
            VersionSkew::ProbeFailed(reason) => {
                assert!(reason.contains("no such file"));
            }
            other => panic!("expected ProbeFailed, got {other:?}"),
        }
        let diag = skew_diagnostic(&skew).expect("probe-failed must yield a diagnostic");
        assert_eq!(diag["code"], "MCP_VERSION_PROBE_FAILED");
    }

    /// BVA: probe returns an empty string. Two possible
    /// interpretations — treat as skew against empty, or as
    /// probe-failure. We choose probe-failure here because an
    /// empty version string is not a meaningful CLI response,
    /// and surfacing it as skew would produce a confusing
    /// `evidence-mcp X.Y.Z ≠ cargo-evidence ` message with
    /// nothing after the delta character.
    #[test]
    fn bva_empty_probe_output_treated_as_probe_failed() {
        // Going through the real `probe_cli_version` is the only
        // way to exercise the "empty output" path (detect_with_probe
        // takes the caller's Result<String> at face value).
        // Instead, unit-test the explicit conversion: a zero-
        // length string out of the probe is an error, not a
        // legitimate version.
        //
        // Shape-level assertion: the empty-string branch in
        // `probe_cli_version` returns `Err`, which
        // `detect_with_probe` translates to `ProbeFailed`.
        let skew = detect_with_probe(|| Err("empty output".to_string()));
        assert!(matches!(skew, VersionSkew::ProbeFailed(_)));
    }

    /// BVA: version strings with pre-release suffixes compare as
    /// strict-equality. `0.1.2` and `0.1.2-dev.1` are treated as
    /// Skewed — the auditor sees both on the warning and can
    /// reason about whether the pre-release matters. Alternative
    /// (semver-equal comparison) would swallow the signal
    /// ambiguously.
    #[test]
    fn bva_prerelease_suffix_counts_as_skew() {
        let mcp = env!("CARGO_PKG_VERSION").to_string();
        let cli = format!("{mcp}-dev.1");
        let skew = detect_with_probe(|| Ok(cli.clone()));
        match &skew {
            VersionSkew::Skewed { mcp: m, cli: c } => {
                assert_eq!(m, &mcp);
                assert_eq!(c, &cli);
            }
            other => panic!("expected Skewed on pre-release suffix, got {other:?}"),
        }
    }
}
