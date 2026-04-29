//! Response-builder helpers shared across the `#[tool]` methods
//! on [`crate::Server`].
//!
//! Split out of `server.rs` to keep the facade under the
//! workspace 500-line limit; the tool-method handlers live in
//! the parent, the functions that translate `RunError` /
//! `VersionSkew` into schema-shaped responses live here. Unit
//! tests for the pure functions (`ping_response_from_skew`) also
//! live here, exercising one variant per branch.

use crate::schema::{JsonlToolResponse, PingResponse, RulesToolResponse};
use crate::subprocess::RunError;
use crate::version_probe::{VersionSkew, skew_diagnostic};
use crate::workspace::{WorkspaceResolution, workspace_fallback_diagnostic};

/// Exit code used for tool-layer subprocess failures. Mirrors
/// the CLI's `EXIT_VERIFICATION_FAILURE` (2) so agents can't
/// tell from `exit_code` alone whether the `evidence` CLI
/// itself failed or the MCP wrapper gave up on the subprocess.
///
/// **`exit_code` is documentation, not the machine contract.**
/// Two failure classes deliberately share `2`:
///
///   1. CLI verification failure — the `cargo evidence` run
///      finished and emitted its own JSONL terminal
///      (`VERIFY_FAIL`, `DOCTOR_FAIL`, `FLOORS_FAIL`).
///   2. MCP tool-layer failure — the wrapper couldn't run the
///      subprocess to completion (cargo not on `PATH`, spawn
///      error, timeout, malformed JSONL output) and synthesized
///      an `MCP_*` terminal in place of the CLI's terminal.
///
/// The structured fields are the canonical machine signal:
///
///   - JSONL verbs (`evidence_check`, `evidence_doctor`,
///     `evidence_floors`) — dispatch on
///     [`JsonlToolResponse::terminal`](crate::schema::JsonlToolResponse).
///   - Rules verb (`evidence_rules`) — dispatch on the `code`
///     field of `error`
///     ([`RulesToolResponse::error`](crate::schema::RulesToolResponse)).
///   - Diff verb (`evidence_diff`) — same `error.code` shape.
///
/// Hosts must not pattern-match on `exit_code` to distinguish
/// the two failure classes — the bit is intentionally erased
/// here. If a future host needs distinct exit semantics, the
/// sharpening would land in a new field, not by sliding the
/// `exit_code` value.
pub(super) const TOOL_FAILURE_EXIT_CODE: i32 = 2;

/// Prepend the synthetic `MCP_WORKSPACE_FALLBACK` diagnostic +
/// bump the `summary` count when the caller fell back to server
/// CWD. No-op for `WorkspaceResolution::Given`. Mutating both
/// vec + map keeps the response self-consistent (agents pattern-
/// matching on either surface see the same count).
pub(super) fn prepend_fallback_signal(
    resolution: WorkspaceResolution,
    cwd: &std::path::Path,
    diagnostics: &mut Vec<serde_json::Value>,
    summary: &mut std::collections::BTreeMap<String, u32>,
) {
    if resolution != WorkspaceResolution::Fallback {
        return;
    }
    diagnostics.insert(0, workspace_fallback_diagnostic(cwd));
    *summary
        .entry("MCP_WORKSPACE_FALLBACK".to_string())
        .or_insert(0) += 1;
}

/// Prepend `MCP_VERSION_SKEW` / `MCP_VERSION_PROBE_FAILED` when
/// the server's cached skew outcome is not `Matched`. No-op on
/// match. Both the diagnostic vec and the summary map get
/// updated so agents reading either surface see the same
/// signal.
pub(super) fn prepend_skew_signal(
    skew: &VersionSkew,
    diagnostics: &mut Vec<serde_json::Value>,
    summary: &mut std::collections::BTreeMap<String, u32>,
) {
    let Some(diag) = skew_diagnostic(skew) else {
        return;
    };
    let code = diag
        .get("code")
        .and_then(|c| c.as_str())
        .unwrap_or("")
        .to_string();
    diagnostics.insert(0, diag);
    if !code.is_empty() {
        *summary.entry(code).or_insert(0) += 1;
    }
}

/// Build a minimal tool-layer diagnostic carrying the given
/// `MCP_*` code + human message at `Severity::Error`. Output
/// shape mirrors `evidence_core::diagnostic::emit_jsonl` so
/// agents cannot tell a tool-layer diagnostic from a CLI-emitted
/// one just by the JSON shape — they pattern-match on `.code`.
pub(super) fn mcp_diagnostic(code: &str, message: &str) -> serde_json::Value {
    serde_json::json!({
        "code": code,
        "severity": "error",
        "message": message,
    })
}

/// Translate a [`RunError`] into a well-formed
/// [`JsonlToolResponse`]. The response carries
/// `exit_code = TOOL_FAILURE_EXIT_CODE`, the matching `MCP_*`
/// code as both `terminal` and the single diagnostic entry, plus
/// the workspace-fallback and version-skew signals when
/// applicable (so agents see every signal — not just the
/// subprocess failure — on a degraded call).
pub(super) fn jsonl_response_from_run_error(
    err: &RunError,
    resolution: WorkspaceResolution,
    cwd: &std::path::Path,
    skew: &VersionSkew,
) -> JsonlToolResponse {
    let code = err.code();
    let message = err.to_string();
    let diag = mcp_diagnostic(code, &message);
    let mut diagnostics = vec![diag];
    let mut summary = std::collections::BTreeMap::new();
    summary.insert(code.to_string(), 1);
    prepend_fallback_signal(resolution, cwd, &mut diagnostics, &mut summary);
    prepend_skew_signal(skew, &mut diagnostics, &mut summary);
    JsonlToolResponse {
        exit_code: TOOL_FAILURE_EXIT_CODE,
        terminal: code.to_string(),
        diagnostics,
        summary,
    }
}

/// Translate a [`RunError`] into a [`RulesToolResponse`] whose
/// `error` field carries the structured diagnostic. `rules` is
/// empty, `count` is 0, `exit_code` is [`TOOL_FAILURE_EXIT_CODE`].
/// `warnings` passes through whatever the caller built (version-
/// skew probe result) so a degraded call still reports both
/// signals.
pub(super) fn rules_response_from_run_error(
    err: &RunError,
    warnings: Vec<serde_json::Value>,
) -> RulesToolResponse {
    let code = err.code();
    let message = err.to_string();
    RulesToolResponse {
        exit_code: TOOL_FAILURE_EXIT_CODE,
        rules: Vec::new(),
        count: 0,
        warnings,
        error: Some(mcp_diagnostic(code, &message)),
    }
}

/// Build a [`PingResponse`] from the cached [`VersionSkew`]. Pure
/// function on the enum — no spawn, no env access, testable in
/// isolation for every variant.
pub(super) fn ping_response_from_skew(skew: &VersionSkew) -> PingResponse {
    let mcp_version = env!("CARGO_PKG_VERSION").to_string();
    match skew {
        VersionSkew::Matched(cli) => PingResponse {
            mcp_version,
            cli_version: Some(cli.clone()),
            skew: "matched".to_string(),
            probe_error: None,
        },
        VersionSkew::Skewed { cli, .. } => PingResponse {
            mcp_version,
            cli_version: Some(cli.clone()),
            skew: "skewed".to_string(),
            probe_error: None,
        },
        VersionSkew::ProbeFailed(reason) => PingResponse {
            mcp_version,
            cli_version: None,
            skew: "probe_failed".to_string(),
            probe_error: Some(reason.clone()),
        },
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

    /// TEST-066 selector: `ping_response_from_skew` maps every
    /// `VersionSkew` variant to the documented `PingResponse`
    /// shape. Pins the field-level contract for each branch.
    #[test]
    fn ping_response_shapes_for_matched_skewed_probe_failed() {
        let mcp = env!("CARGO_PKG_VERSION").to_string();

        let matched = ping_response_from_skew(&VersionSkew::Matched(mcp.clone()));
        assert_eq!(matched.mcp_version, mcp);
        assert_eq!(matched.cli_version.as_deref(), Some(mcp.as_str()));
        assert_eq!(matched.skew, "matched");
        assert!(matched.probe_error.is_none());

        let skewed = ping_response_from_skew(&VersionSkew::Skewed {
            mcp: mcp.clone(),
            cli: "0.0.1-stale".to_string(),
        });
        assert_eq!(skewed.mcp_version, mcp);
        assert_eq!(skewed.cli_version.as_deref(), Some("0.0.1-stale"));
        assert_eq!(skewed.skew, "skewed");
        assert!(skewed.probe_error.is_none());

        let failed = ping_response_from_skew(&VersionSkew::ProbeFailed("no such file".to_string()));
        assert_eq!(failed.mcp_version, mcp);
        assert!(failed.cli_version.is_none());
        assert_eq!(failed.skew, "probe_failed");
        assert_eq!(failed.probe_error.as_deref(), Some("no such file"));
    }
}
