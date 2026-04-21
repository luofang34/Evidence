//! `mcp-evidence` — stdio MCP server exposing `cargo evidence`
//! agent verbs to AI hosts (Claude Desktop, Claude Code, etc).
//!
//! Stdio transport is strict: stdout is the MCP protocol channel;
//! everything informational (tracing, panics, panics from deeper
//! crates) must go to stderr. `init_tracing` binds
//! `tracing_subscriber` to stderr so `tracing::info!` / `warn!` /
//! `error!` calls from `evidence_core` or dependent crates don't
//! corrupt the protocol.

#![allow(
    clippy::disallowed_types,
    reason = "main uses anyhow::Result as the conventional CLI envelope"
)]

use mcp_evidence::Server;
use rmcp::{ServiceExt, transport::io::stdio};
use tracing_subscriber::EnvFilter;

fn init_tracing() {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn")),
        )
        .with_target(false)
        .init();
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();
    let service = Server::default().serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
