//! MCP server exposing `cargo evidence` agent verbs over stdio.
//!
//! This crate is a thin subprocess wrapper: each MCP tool call spawns
//! `cargo evidence <verb> --format=jsonl` (or `--json` for `rules`
//! which is blob-only), collects stdout + the exit code, and returns
//! a structured response that agents can pattern-match on.
//!
//! The CLI's JSONL output shape is the stable contract (tested by
//! `crates/cargo-evidence/tests/verify_jsonl.rs` and siblings).
//! `mcp_evidence` does not introduce new diagnostic codes — every
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

use schema::{RulesRequest, RulesToolResponse};
use subprocess::run_evidence;

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
        let cwd = std::env::current_dir()
            .map_err(|e| format!("cannot resolve server CWD: {e}"))?;
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
}

impl Default for Server {
    fn default() -> Self {
        Self::new()
    }
}

/// Resolve an optional workspace-path request field against the
/// server's CWD. Absolute paths pass through; relative paths are
/// joined onto `current_dir`. The resolved path is returned
/// without canonicalization — the CLI itself handles existence
/// checks and emits structured errors on missing paths.
///
/// Commit 4 (doctor) and commit 5 (check) consume this helper.
#[allow(dead_code, reason = "consumed by commits 4 and 5 (doctor + check)")]
pub(crate) fn resolve_workspace(path: Option<&str>) -> Result<PathBuf, String> {
    let cwd = std::env::current_dir()
        .map_err(|e| format!("cannot resolve server CWD: {e}"))?;
    match path {
        None => Ok(cwd),
        Some(p) => {
            let requested = PathBuf::from(p);
            if requested.is_absolute() {
                Ok(requested)
            } else {
                Ok(cwd.join(requested))
            }
        }
    }
}
