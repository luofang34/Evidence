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

use rmcp::{ServerHandler, handler::server::router::tool::ToolRouter, tool_handler, tool_router};

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
    /// Build a fresh server handle. Registers the three tool methods
    /// (currently none — commits 3–5 add `evidence_rules`,
    /// `evidence_doctor`, `evidence_check` in that order).
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }
}

impl Default for Server {
    fn default() -> Self {
        Self::new()
    }
}
