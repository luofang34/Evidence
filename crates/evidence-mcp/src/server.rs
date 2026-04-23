//! MCP server handle + tool-method impls. Split from `lib.rs` so
//! the facade stays under the 80-line target; groups all
//! `ServerHandler` / `#[tool_router]` concerns in one module.
//!
//! `name = "evidence-mcp"` on the `#[tool_handler]` attribute is
//! load-bearing: rmcp's default `from_build_env()` identifies the
//! server as `rmcp`, making evidence-mcp indistinguishable from
//! any other rmcp-built server in the `initialize` response.
//! Agents pattern-matching on `serverInfo.name` need the tool's
//! real identity — LLR-062 pins it.

use std::sync::Arc;

use rmcp::{
    Json, ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    tool, tool_handler, tool_router,
};

use crate::schema::{
    CheckRequest, DoctorRequest, JsonlToolResponse, PingRequest, PingResponse, RulesRequest,
    RulesToolResponse,
};
use crate::subprocess::{MCP_MALFORMED_JSONL, RunError, parse_jsonl, run_evidence};
use crate::version_probe::{VersionSkew, detect_with_probe, probe_cli_version, skew_diagnostic};
use crate::workspace::{WorkspaceResolution, resolve_workspace, workspace_fallback_diagnostic};

/// Exit code used for tool-layer subprocess failures. Mirrors
/// the CLI's `EXIT_VERIFICATION_FAILURE` (2) so agents can't
/// tell from `exit_code` alone whether the `evidence` CLI
/// itself failed or the MCP wrapper gave up on the subprocess.
const TOOL_FAILURE_EXIT_CODE: i32 = 2;

/// MCP server handle. Stateless per-request — each tool call
/// resolves its workspace path independently and spawns a fresh
/// subprocess. The one piece of server-lifetime state is
/// `version_skew`, captured once at [`Server::new`] via a
/// startup probe of `cargo evidence --version`; each tool
/// response prepends a warning diagnostic when the probe
/// detected a mismatch.
///
/// Construct via [`Server::new`] (or `Default::default`) and call
/// `server.serve(rmcp::transport::io::stdio()).await?.waiting().await?`
/// in a `#[tokio::main]` context to run the stdio loop.
#[derive(Debug, Clone)]
pub struct Server {
    tool_router: ToolRouter<Self>,
    /// Cached outcome of the `cargo evidence --version` probe.
    /// `Arc` so `#[derive(Clone)]` (required by rmcp) doesn't
    /// copy the strings per-request.
    version_skew: Arc<VersionSkew>,
}

// `name = "evidence-mcp"` overrides rmcp's default
// `from_build_env()` identity (which reads rmcp's own crate env
// and shows up as "rmcp" in `initialize` responses). Version
// falls through to `env!("CARGO_PKG_VERSION")` at macro-
// expansion time, reading evidence-mcp's package version.
#[tool_handler(router = self.tool_router, name = "evidence-mcp")]
impl ServerHandler for Server {}

#[tool_router(router = tool_router)]
impl Server {
    /// Build a fresh server handle. Registers the tool methods on
    /// construction via the `ToolRouter::new`-generated factory.
    /// Also runs the one-shot version probe so subsequent tool
    /// responses can prepend the `MCP_VERSION_SKEW` /
    /// `MCP_VERSION_PROBE_FAILED` warning without paying the
    /// subprocess-spawn cost per call.
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
            version_skew: Arc::new(detect_with_probe(probe_cli_version)),
        }
    }

    /// `evidence_rules` — return the full manifest of diagnostic
    /// codes the tool can emit.
    ///
    /// Thin pass-through over `cargo evidence rules --json`. The
    /// response carries the raw manifest, an `exit_code`, and a
    /// convenience `count` agents can pin against
    /// `evidence_core::RULES.len()` to detect CLI-vs-library drift.
    #[tool(
        name = "evidence_rules",
        description = "List every diagnostic code cargo-evidence can emit (self-describe manifest). \
                       Useful for agents building autofix flows — pin response codes against \
                       the returned list to avoid guessing at the vocabulary."
    )]
    pub async fn evidence_rules(
        &self,
        _params: Parameters<RulesRequest>,
    ) -> Result<Json<RulesToolResponse>, String> {
        let cwd = std::env::current_dir().map_err(|e| format!("cannot resolve server CWD: {e}"))?;
        let warnings = skew_diagnostic(&self.version_skew)
            .map(|d| vec![d])
            .unwrap_or_default();
        let captured = match run_evidence(&["rules", "--json"], &cwd).await {
            Ok(c) => c,
            Err(e) => return Ok(Json(rules_response_from_run_error(&e, warnings))),
        };
        match serde_json::from_slice::<Vec<serde_json::Value>>(&captured.stdout) {
            Ok(rules) => {
                let count = rules.len();
                Ok(Json(RulesToolResponse {
                    exit_code: captured.exit_code,
                    rules,
                    count,
                    warnings,
                    error: None,
                }))
            }
            Err(e) => Ok(Json(RulesToolResponse {
                exit_code: TOOL_FAILURE_EXIT_CODE,
                rules: Vec::new(),
                count: 0,
                warnings,
                error: Some(mcp_diagnostic(
                    MCP_MALFORMED_JSONL,
                    &format!("cargo evidence rules --json produced invalid JSON: {e}"),
                )),
            })),
        }
    }

    /// `evidence_doctor` — audit a workspace's rigor adoption.
    ///
    /// Runs `cargo evidence doctor --format=jsonl` in the
    /// requested workspace (or the server's CWD if omitted) and
    /// returns the per-check diagnostics plus the `DOCTOR_OK` /
    /// `DOCTOR_FAIL` terminal. Agents use this to gate generate-
    /// cert invocations on downstream projects — all six checks
    /// (boundary config, trace validity, floors config, CI
    /// integration, merge-style policy, override-docs) must pass
    /// or cert profile refuses.
    #[tool(
        name = "evidence_doctor",
        description = "Audit a workspace's rigor adoption: trace validity, floors config, \
                       boundary config, CI integration, merge-style policy, override-protocol \
                       docs. Returns exit_code 0 on DOCTOR_OK (no error-severity findings), 2 \
                       on DOCTOR_FAIL (one or more blockers). Warnings still return exit_code 0 \
                       under standalone invocation; cert-profile generate escalates warnings \
                       separately."
    )]
    pub async fn evidence_doctor(
        &self,
        Parameters(req): Parameters<DoctorRequest>,
    ) -> Result<Json<JsonlToolResponse>, String> {
        let (cwd, resolution) = resolve_workspace(req.workspace_path.as_deref())?;
        let captured = match run_evidence(&["doctor", "--format=jsonl"], &cwd).await {
            Ok(c) => c,
            Err(e) => {
                return Ok(Json(jsonl_response_from_run_error(
                    &e,
                    resolution,
                    &cwd,
                    &self.version_skew,
                )));
            }
        };
        let (terminal, mut diagnostics, mut summary) = parse_jsonl(&captured.stdout);
        prepend_fallback_signal(resolution, &cwd, &mut diagnostics, &mut summary);
        prepend_skew_signal(&self.version_skew, &mut diagnostics, &mut summary);
        Ok(Json(JsonlToolResponse {
            exit_code: captured.exit_code,
            terminal,
            diagnostics,
            summary,
        }))
    }

    /// `evidence_check` — one-shot pass/gap validation of a
    /// workspace (source tree) or an evidence bundle.
    ///
    /// Wraps `cargo evidence check --format=jsonl
    /// [--mode <auto|source|bundle>]`. Source mode spawns
    /// `cargo test --workspace` under the hood and can take
    /// several minutes on large workspaces; the spawn is bounded
    /// by the `subprocess::spawn_timeout` cap — 10 minutes by
    /// default, tunable via `EVIDENCE_MCP_TIMEOUT_SECS`.
    ///
    /// `--mode=source` executes workspace tests and therefore
    /// carries whatever side-effects those tests have (file
    /// writes under the workspace, bound sockets, env mutations,
    /// subprocess spawns). `--mode=bundle` is pure inspection —
    /// it reads the bundle directory and verifies SHA256SUMS
    /// without executing project code.
    ///
    /// Agents use this as their primary validation call; `verify`
    /// is intentionally NOT exposed over MCP — `check` in bundle
    /// mode delegates to `verify` internally.
    #[tool(
        name = "evidence_check",
        description = "One-shot pass/gap validation of a workspace or bundle. Spawns \
                       `cargo evidence check --format=jsonl`; streams one REQ_PASS / \
                       REQ_GAP / REQ_SKIP per requirement then terminates with VERIFY_OK \
                       (exit 0) or VERIFY_FAIL (exit 2). \
                       `--mode=source` EXECUTES the workspace's tests (`cargo test \
                       --workspace`): can take several minutes and carries the usual \
                       test-side-effects (file writes under the workspace, bound sockets, \
                       env mutations, spawned processes). `--mode=bundle` is inspection- \
                       only — reads the bundle directory and delegates to the `verify` \
                       pipeline without executing project code. `--mode=auto` (default) \
                       picks source or bundle based on the marker file at the given path."
    )]
    pub async fn evidence_check(
        &self,
        Parameters(req): Parameters<CheckRequest>,
    ) -> Result<Json<JsonlToolResponse>, String> {
        let (cwd, resolution) = resolve_workspace(req.workspace_path.as_deref())?;
        let mut args: Vec<String> = vec!["check".into(), "--format=jsonl".into()];
        if let Some(mode) = req.mode.as_deref() {
            // Validate the mode up front to give the agent a
            // clean error instead of shipping nonsense to the CLI.
            match mode {
                "auto" | "source" | "bundle" => {
                    args.push(format!("--mode={mode}"));
                }
                other => {
                    return Err(format!(
                        "invalid mode {other:?}; expected one of \"auto\" | \"source\" | \"bundle\""
                    ));
                }
            }
        }
        let args_refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let captured = match run_evidence(&args_refs, &cwd).await {
            Ok(c) => c,
            Err(e) => {
                return Ok(Json(jsonl_response_from_run_error(
                    &e,
                    resolution,
                    &cwd,
                    &self.version_skew,
                )));
            }
        };
        let (terminal, mut diagnostics, mut summary) = parse_jsonl(&captured.stdout);
        prepend_fallback_signal(resolution, &cwd, &mut diagnostics, &mut summary);
        prepend_skew_signal(&self.version_skew, &mut diagnostics, &mut summary);
        Ok(Json(JsonlToolResponse {
            exit_code: captured.exit_code,
            terminal,
            diagnostics,
            summary,
        }))
    }

    /// `evidence_ping` — cheap liveness + version-skew probe.
    ///
    /// Takes no arguments. Returns the cached `VersionSkew`
    /// captured at `Server::new()` packaged as [`PingResponse`] — no
    /// subprocess spawn per call. Hosts use this as a reachability
    /// check before issuing expensive verbs (`evidence_check
    /// --mode=source` can run `cargo test --workspace` for
    /// minutes); ping is fast enough to call on every agent loop
    /// iteration.
    #[tool(
        name = "evidence_ping",
        description = "Cheap liveness + version-skew probe. Returns evidence-mcp's version, \
                       the cached cargo-evidence version from the startup probe, and a skew \
                       tag (matched / skewed / probe_failed). Does NOT spawn a subprocess per \
                       call — the handler reads the VersionSkew captured at server startup. \
                       Use as a reachability check before invoking evidence_check in source \
                       mode or other expensive verbs."
    )]
    pub async fn evidence_ping(
        &self,
        _params: Parameters<PingRequest>,
    ) -> Result<Json<PingResponse>, String> {
        Ok(Json(ping_response_from_skew(&self.version_skew)))
    }
}

impl Default for Server {
    fn default() -> Self {
        Self::new()
    }
}

/// Prepend the synthetic `MCP_WORKSPACE_FALLBACK` diagnostic +
/// bump the `summary` count when the caller fell back to server
/// CWD. No-op for `WorkspaceResolution::Given`. Mutating both
/// vec + map keeps the response self-consistent (agents pattern-
/// matching on either surface see the same count).
fn prepend_fallback_signal(
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
fn prepend_skew_signal(
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
fn mcp_diagnostic(code: &str, message: &str) -> serde_json::Value {
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
fn jsonl_response_from_run_error(
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
fn rules_response_from_run_error(
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
fn ping_response_from_skew(skew: &VersionSkew) -> PingResponse {
    let mcp_version = env!("CARGO_PKG_VERSION").to_string();
    match skew {
        VersionSkew::Matched => PingResponse {
            mcp_version: mcp_version.clone(),
            cli_version: Some(mcp_version),
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

        let matched = ping_response_from_skew(&VersionSkew::Matched);
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
