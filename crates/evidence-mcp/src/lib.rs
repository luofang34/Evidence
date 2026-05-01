//! MCP server exposing `cargo evidence` agent verbs over stdio.
//!
//! This crate is a thin subprocess wrapper: each MCP tool call spawns
//! `cargo evidence <verb> --format=jsonl` (or `--json` for `rules`
//! which is blob-only), collects stdout + the exit code, and returns
//! a structured response that agents can pattern-match on.
//!
//! The CLI's JSONL output shape is the stable contract (tested by
//! `crates/cargo-evidence/tests/verify_jsonl.rs` and siblings). Codes
//! minted by the MCP wrapper itself (the workspace-fallback signal,
//! the version-skew warnings, the subprocess-wrapper failures, and
//! the JSONL parse terminals) carry the `MCP_` prefix and are
//! registered in [`evidence_core::HAND_EMITTED_MCP_CODES`].
//! Everything else comes from the CLI's stream.
//!
//! Tool-layer failure modes — `cargo` not on PATH, spawn error,
//! subprocess timeout, malformed JSONL, empty stdout — surface as a
//! well-formed [`schema::JsonlToolResponse`] or
//! [`schema::RulesToolResponse`] carrying `exit_code == 2` and a
//! single structured diagnostic whose `.code` is the matching
//! `MCP_*` string. `Err(String)` from a tool method is reserved for
//! host-contract breakages (server CWD unresolvable, invalid enum
//! value for `mode`).
//!
//! See [`Server`] for the handler + tool methods. See SYS-018 /
//! HLR-050 / LLR-050 / TEST-050 in `cert/trace/` for the requirements
//! chain behind this crate; LLR-062 covers the `serverInfo` identity
//! override and the `lib.rs`-as-facade split; LLR-063 covers the
//! version-skew probe; LLR-064 pins the structured tool-layer
//! failure contract and the `HAND_EMITTED_MCP_CODES` registry.
//!
//! [`evidence_core::HAND_EMITTED_MCP_CODES`]: evidence_core::HAND_EMITTED_MCP_CODES

pub mod schema;
mod server;
mod subprocess;
mod version_probe;
mod workspace;

pub use server::Server;
