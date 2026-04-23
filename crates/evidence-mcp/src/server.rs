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
    CheckRequest, DoctorRequest, FloorsRequest, JsonlToolResponse, PingRequest, PingResponse,
    RulesRequest, RulesToolResponse,
};
use crate::subprocess::{MCP_MALFORMED_JSONL, parse_jsonl, run_evidence};
use crate::version_probe::{VersionSkew, detect_with_probe, probe_cli_version, skew_diagnostic};
use crate::workspace::resolve_workspace;

mod responses;
use responses::{
    TOOL_FAILURE_EXIT_CODE, jsonl_response_from_run_error, mcp_diagnostic, ping_response_from_skew,
    prepend_fallback_signal, prepend_skew_signal, rules_response_from_run_error,
};

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

    /// `evidence_floors` — query the ratchet-gate state of a
    /// workspace.
    ///
    /// Wraps `cargo evidence floors --format=jsonl`. Streams one
    /// `FLOORS_DIMENSION_OK` / `FLOORS_BELOW_MIN` per measured
    /// dimension and terminates with `FLOORS_OK` (all dimensions
    /// satisfied) or `FLOORS_FAIL` (at least one below committed
    /// floor). Agents use this to diagnose a red ratchet gate on
    /// a PR without cloning and building — the structured stream
    /// names each failing dimension so the agent can suggest
    /// targeted fixes.
    #[tool(
        name = "evidence_floors",
        description = "Query the ratchet-gate state of a workspace: measures every dimension \
                       in `cert/floors.toml` and reports which ones are below their committed \
                       floor. Spawns `cargo evidence floors --format=jsonl`; streams one \
                       FLOORS_DIMENSION_OK / FLOORS_BELOW_MIN per dimension then terminates \
                       with FLOORS_OK (exit 0) or FLOORS_FAIL (exit 2). Pure inspection — \
                       does not execute project code."
    )]
    pub async fn evidence_floors(
        &self,
        Parameters(req): Parameters<FloorsRequest>,
    ) -> Result<Json<JsonlToolResponse>, String> {
        let (cwd, resolution) = resolve_workspace(req.workspace_path.as_deref())?;
        let captured = match run_evidence(&["floors", "--format=jsonl"], &cwd).await {
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

    /// `evidence_diff` — compare two evidence bundles on-disk
    /// and return the structured delta across inputs, outputs,
    /// metadata, and env.
    ///
    /// Wraps `cargo evidence diff <a> <b> --json`. Pure
    /// inspection — reads both bundle directories, computes the
    /// delta. Unlike the streaming verbs, the CLI emits a single
    /// JSON blob here, so the response carries the raw `diff`
    /// document under the `diff` field (`None` on tool-layer
    /// failure) rather than a `terminal` + `diagnostics` +
    /// `summary` triad.
    #[tool(
        name = "evidence_diff",
        description = "Compare two evidence bundles on-disk. Wraps `cargo evidence diff \
                       <a> <b> --json`; returns the structured delta across inputs, outputs, \
                       metadata, and env as a single JSON blob. Pure inspection — does not \
                       execute project code. Both bundle_a_path and bundle_b_path are \
                       required; diff has no workspace default."
    )]
    pub async fn evidence_diff(
        &self,
        Parameters(req): Parameters<crate::schema::DiffRequest>,
    ) -> Result<Json<crate::schema::DiffToolResponse>, String> {
        let cwd = std::env::current_dir().map_err(|e| format!("cannot resolve server CWD: {e}"))?;
        let warnings = skew_diagnostic(&self.version_skew)
            .map(|d| vec![d])
            .unwrap_or_default();
        let captured = match run_evidence(
            &["diff", &req.bundle_a_path, &req.bundle_b_path, "--json"],
            &cwd,
        )
        .await
        {
            Ok(c) => c,
            Err(e) => {
                return Ok(Json(crate::schema::DiffToolResponse {
                    exit_code: TOOL_FAILURE_EXIT_CODE,
                    diff: None,
                    warnings,
                    error: Some(mcp_diagnostic(e.code(), &e.to_string())),
                }));
            }
        };
        match serde_json::from_slice::<serde_json::Value>(&captured.stdout) {
            Ok(diff) => Ok(Json(crate::schema::DiffToolResponse {
                exit_code: captured.exit_code,
                diff: Some(diff),
                warnings,
                error: None,
            })),
            Err(e) => Ok(Json(crate::schema::DiffToolResponse {
                exit_code: TOOL_FAILURE_EXIT_CODE,
                diff: None,
                warnings,
                error: Some(mcp_diagnostic(
                    MCP_MALFORMED_JSONL,
                    &format!("cargo evidence diff --json produced invalid JSON: {e}"),
                )),
            })),
        }
    }
}

impl Default for Server {
    fn default() -> Self {
        Self::new()
    }
}
