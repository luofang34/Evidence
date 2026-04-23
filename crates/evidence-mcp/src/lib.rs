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
//! chain behind this crate; LLR-062 covers the `serverInfo` identity
//! override and the `lib.rs`-as-facade split.

pub mod schema;
mod server;
mod subprocess;
mod workspace;

pub use server::Server;
