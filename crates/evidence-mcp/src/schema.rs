//! Tool input/output schemas, shared across the three `#[tool]`
//! methods on [`crate::Server`].
//!
//! The `JsonSchema` derive is what rmcp reads to advertise tool
//! argument shapes to agents. Field-level doc comments become the
//! JSON Schema `description` field per schemars convention — they
//! are load-bearing and worth keeping specific.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Empty-input marker for `evidence_rules` (which takes no
/// arguments). A typed empty struct is friendlier to agent
/// hosts than omitting the parameter wrapper entirely — they get
/// a schema with `"properties": {}` rather than a missing
/// `inputSchema`.
///
/// `#[serde(deny_unknown_fields)]` is defense-in-depth: an
/// agent that mistakenly ships a `workspace_path` (which
/// `evidence_rules` doesn't accept) gets a clear error rather
/// than having the field silently dropped. Required by
/// HLR-054 / LLR-054.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RulesRequest {}

/// Input to `evidence_check`.
///
/// `#[serde(deny_unknown_fields)]` prevents agent typos from
/// silently falling through to server-CWD defaults. A request
/// like `{"workspace": "/path"}` (note the missing `_path`
/// suffix) produces a serde error rather than running against
/// the server's CWD.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CheckRequest {
    /// Absolute or MCP-server-CWD-relative path to the workspace
    /// root. For source mode the directory must contain
    /// `Cargo.toml`; for bundle mode it must contain `SHA256SUMS`.
    /// Defaults to the server's CWD when omitted.
    #[serde(default)]
    pub workspace_path: Option<String>,

    /// Mirrors `cargo evidence check --mode`. One of `"auto"`,
    /// `"source"`, `"bundle"`. Defaults to `"auto"` — the CLI
    /// inspects the path and picks source or bundle based on
    /// which marker file it finds.
    #[serde(default)]
    pub mode: Option<String>,
}

/// Input to `evidence_doctor`.
///
/// `#[serde(deny_unknown_fields)]` — see [`CheckRequest`] for
/// the agent-typo rationale.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DoctorRequest {
    /// Absolute or MCP-server-CWD-relative path to the workspace
    /// to audit. Defaults to the server's CWD when omitted.
    #[serde(default)]
    pub workspace_path: Option<String>,
}

/// Shared response shape for `evidence_check` and
/// `evidence_doctor` — both emit a JSONL stream terminated by a
/// `_OK`/`_FAIL`/`_ERROR` diagnostic.
///
/// `diagnostics` holds every parsed stdout line as opaque JSON
/// values rather than typed `evidence_core::Diagnostic`. This
/// avoids forcing `schemars::JsonSchema` onto the core type and
/// sidesteps a re-serialize cycle — agents pattern-match on
/// `.code` anyway.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct JsonlToolResponse {
    /// Process exit code from the spawned `cargo evidence` run.
    /// `0` = success, `1` = runtime/argument error, `2` =
    /// verification failure.
    pub exit_code: i32,

    /// `code` of the last (terminal) diagnostic in the stream.
    /// One of `evidence_core::TERMINAL_CODES` on a well-formed
    /// run (`VERIFY_OK`, `VERIFY_FAIL`, `VERIFY_ERROR`,
    /// `DOCTOR_OK`, `DOCTOR_FAIL`, `CLI_SUBCOMMAND_ERROR`).
    /// Synthesized to `"NO_OUTPUT"` if the CLI emitted nothing.
    pub terminal: String,

    /// Every parsed JSONL line from the run, in stream order.
    /// Each entry is a `Diagnostic`-shaped object as rendered by
    /// `evidence_core::diagnostic::emit_jsonl`.
    pub diagnostics: Vec<serde_json::Value>,

    /// Frequency map over `.code` values for quick agent-side
    /// pattern-matching: `{"REQ_PASS": 164, "REQ_GAP": 0, ...}`.
    pub summary: BTreeMap<String, u32>,
}

/// Response shape for `evidence_rules` — a one-shot dump of the
/// tool's diagnostic-code manifest.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct RulesToolResponse {
    /// Exit code from `cargo evidence rules --json`. Should be 0
    /// on any successful run; non-zero signals a CLI bug.
    pub exit_code: i32,

    /// The full rules manifest as emitted by the CLI — an array
    /// of `{code, severity, domain, has_fix_hint, terminal}`
    /// objects, alphabetically sorted by `code`.
    pub rules: Vec<serde_json::Value>,

    /// Convenience: `rules.len()`. Agents can pin this against
    /// `evidence_core::RULES.len()` for a drift check without
    /// deserializing every entry.
    pub count: usize,

    /// Server-layer warnings synthesized by the MCP wrapper, not
    /// by the underlying CLI. Carries `MCP_VERSION_SKEW` /
    /// `MCP_VERSION_PROBE_FAILED` when a mismatch between
    /// `evidence-mcp` and the spawned `cargo-evidence` is
    /// detected. Empty in the happy path. Separate from `rules`
    /// so an agent consuming the manifest doesn't have to
    /// filter out server signals.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<serde_json::Value>,
}
