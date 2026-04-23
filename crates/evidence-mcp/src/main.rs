//! `evidence-mcp` — stdio MCP server exposing `cargo evidence`
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

use evidence_mcp::Server;
use rmcp::{ServiceExt, transport::io::stdio};
use tracing::level_filters::LevelFilter;
use tracing_subscriber::EnvFilter;

fn init_tracing() {
    // `from_env_lossy` preserves valid directives when RUST_LOG
    // has a syntax error elsewhere in the string, and honors an
    // empty RUST_LOG="" (which some sandboxes set unconditionally,
    // defeating `try_from_default_env` and dropping into the
    // fallback). `with_default_directive` supplies the baseline
    // when the env var is unset or empty.
    let filter = EnvFilter::builder()
        .with_default_directive(LevelFilter::WARN.into())
        .from_env_lossy();
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(filter)
        .with_target(false)
        .init();
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Intercept `--version` / `--help` before the stdio transport
    // takes over. Without this, `evidence-mcp --version` hangs on
    // the MCP initialize handshake and eventually returns the
    // cryptic `connection closed: initialize request` — wrong
    // answer to "which version did I just install?".
    if let Some(flag) = std::env::args().nth(1) {
        match flag.as_str() {
            "-V" | "--version" => {
                println!("evidence-mcp {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "-h" | "--help" => {
                println!(
                    "evidence-mcp {} — MCP server exposing `cargo evidence` to AI agents.\n\
                     \n\
                     This binary speaks the Model Context Protocol over stdio and is\n\
                     intended to be launched by an MCP host (Claude Desktop, Claude\n\
                     Code, etc.), not invoked directly from the shell.\n\
                     \n\
                     Host-registration examples:\n\
                     \n\
                       Claude Code:    claude mcp add evidence evidence-mcp\n\
                       Claude Desktop: see crates.io/crates/evidence-mcp\n\
                     \n\
                     Flags:\n\
                       -V, --version    Print version and exit\n\
                       -h, --help       Print this help and exit",
                    env!("CARGO_PKG_VERSION")
                );
                return Ok(());
            }
            _ => {}
        }
    }
    init_tracing();
    let service = Server::default().serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
