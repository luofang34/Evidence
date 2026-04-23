//! MCP server exposing `cargo evidence` agent verbs over stdio.
//!
//! This crate is a thin subprocess wrapper: each MCP tool call spawns
//! `cargo evidence <verb> --format=jsonl` (or `--json` for `rules`
//! which is blob-only), collects stdout + the exit code, and returns
//! a structured response that agents can pattern-match on.
//!
//! The CLI's JSONL output shape is the stable contract (tested by
//! `crates/cargo-evidence/tests/verify_jsonl.rs` and siblings).
//! `evidence_mcp` does not introduce new diagnostic codes — every
//! `.code` string in a tool response already exists in
//! `evidence_core::RULES`. Tool-layer failure signals
//! (`BinaryNotFound`, `MalformedJsonl`, `CHECK_TIMEOUT`) surface as
//! structured errors in the tool response, not as new public codes.
//!
//! See [`Server`] for the handler + tool methods. See SYS-018 /
//! HLR-050 / LLR-050 / TEST-050 in `tool/trace/` for the requirements
//! chain behind this crate.

use std::path::PathBuf;

use rmcp::{
    Json, ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    tool, tool_handler, tool_router,
};

pub mod schema;
mod subprocess;

use schema::{CheckRequest, DoctorRequest, JsonlToolResponse, RulesRequest, RulesToolResponse};
use subprocess::{parse_jsonl, run_evidence};

/// MCP server handle. Stateless — every tool call resolves its
/// workspace path independently and spawns a fresh subprocess.
///
/// Construct via [`Server::new`] (or `Default::default`) and call
/// `server.serve(rmcp::transport::io::stdio()).await?.waiting().await?`
/// in a `#[tokio::main]` context to run the stdio loop.
#[derive(Debug, Clone)]
pub struct Server {
    tool_router: ToolRouter<Self>,
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for Server {}

#[tool_router(router = tool_router)]
impl Server {
    /// Build a fresh server handle. Registers the tool methods on
    /// construction via the `ToolRouter::new`-generated factory.
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
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
        let captured = run_evidence(&["rules", "--json"], &cwd)
            .await
            .map_err(|e| e.to_string())?;
        let rules: Vec<serde_json::Value> = serde_json::from_slice(&captured.stdout)
            .map_err(|e| format!("cargo evidence rules --json produced invalid JSON: {e}"))?;
        let count = rules.len();
        Ok(Json(RulesToolResponse {
            exit_code: captured.exit_code,
            rules,
            count,
        }))
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
        let captured = run_evidence(&["doctor", "--format=jsonl"], &cwd)
            .await
            .map_err(|e| e.to_string())?;
        let (terminal, mut diagnostics, mut summary) = parse_jsonl(&captured.stdout);
        prepend_fallback_signal(resolution, &cwd, &mut diagnostics, &mut summary);
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
    /// by a 10-minute timeout (`SPAWN_TIMEOUT` in the
    /// `subprocess` module).
    ///
    /// Agents use this as their primary validation call; `verify`
    /// is intentionally NOT exposed over MCP — `check` in bundle
    /// mode delegates to `verify` internally.
    #[tool(
        name = "evidence_check",
        description = "One-shot pass/gap validation of a workspace or bundle. Spawns \
                       `cargo evidence check --format=jsonl`; streams one REQ_PASS / \
                       REQ_GAP / REQ_SKIP per requirement then terminates with VERIFY_OK \
                       (exit 0) or VERIFY_FAIL (exit 2). Source mode runs `cargo test \
                       --workspace` — can take several minutes. Bundle mode delegates to \
                       the `verify` pipeline."
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
        let captured = run_evidence(&args_refs, &cwd)
            .await
            .map_err(|e| e.to_string())?;
        let (terminal, mut diagnostics, mut summary) = parse_jsonl(&captured.stdout);
        prepend_fallback_signal(resolution, &cwd, &mut diagnostics, &mut summary);
        Ok(Json(JsonlToolResponse {
            exit_code: captured.exit_code,
            terminal,
            diagnostics,
            summary,
        }))
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

impl Default for Server {
    fn default() -> Self {
        Self::new()
    }
}

/// Classification of how a tool call's working directory was
/// chosen, returned alongside the resolved path by
/// [`resolve_workspace`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WorkspaceResolution {
    /// The request supplied an explicit `workspace_path`. No
    /// agent-facing signal is needed.
    Given,
    /// The request omitted `workspace_path`; the handler fell
    /// back to the server's CWD. Callers that produce a
    /// user-visible diagnostic stream prepend a
    /// `MCP_WORKSPACE_FALLBACK` Warning so an agent typo of
    /// "omitted" vs an intentional no-argument call is
    /// distinguishable from the response (HLR-054 / LLR-054).
    Fallback,
}

/// Resolve an optional workspace-path request field against the
/// server's CWD. Absolute paths pass through; relative paths are
/// joined onto `current_dir`. The resolved path is returned
/// without canonicalization — the CLI itself handles existence
/// checks and emits structured errors on missing paths. The
/// [`WorkspaceResolution`] return value distinguishes an
/// explicit path from a CWD fallback so the caller can emit
/// `MCP_WORKSPACE_FALLBACK` on the `Fallback` arm.
pub(crate) fn resolve_workspace(
    path: Option<&str>,
) -> Result<(PathBuf, WorkspaceResolution), String> {
    let cwd = std::env::current_dir().map_err(|e| format!("cannot resolve server CWD: {e}"))?;
    match path {
        None => Ok((cwd, WorkspaceResolution::Fallback)),
        Some(p) => {
            let requested = PathBuf::from(p);
            let resolved = if requested.is_absolute() {
                requested
            } else {
                cwd.join(requested)
            };
            Ok((resolved, WorkspaceResolution::Given))
        }
    }
}

/// Build a synthetic `MCP_WORKSPACE_FALLBACK` Warning diagnostic
/// shaped like the JSONL entries the CLI emits, so agents can
/// pattern-match on `.code` uniformly across both MCP-layer and
/// CLI-layer diagnostics. `cwd` is embedded in the message so
/// the agent sees which directory actually ran — turning a
/// silent fallback into an observable contract (HLR-054).
fn workspace_fallback_diagnostic(cwd: &std::path::Path) -> serde_json::Value {
    serde_json::json!({
        "code": "MCP_WORKSPACE_FALLBACK",
        "severity": "warning",
        "message": format!(
            "workspace_path omitted; using MCP server CWD {:?}. \
             Pass an explicit workspace_path to silence this warning.",
            cwd
        ),
        "subcommand": "mcp",
    })
}
