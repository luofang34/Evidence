//! Tool input/output schemas, shared across the `#[tool]` methods
//! on [`crate::Server`].
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
///
/// Tool-layer failures (subprocess couldn't spawn, timed out,
/// produced nothing, or produced malformed JSONL) appear as a
/// single synthesized `MCP_*` diagnostic with matching
/// `terminal` and `exit_code == 2`, not as an rmcp `Err`.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct JsonlToolResponse {
    /// Process exit code advertised back to the host. `0` on
    /// success, `1` on runtime/argument error from the CLI, `2`
    /// on verification failure OR on tool-layer subprocess
    /// failure (in which case `terminal` carries an `MCP_*`
    /// code).
    pub exit_code: i32,

    /// `code` of the last (terminal) diagnostic in the stream.
    /// One of `evidence_core::TERMINAL_CODES` on a well-formed
    /// run (`VERIFY_OK`, `VERIFY_FAIL`, `VERIFY_ERROR`,
    /// `DOCTOR_OK`, `DOCTOR_FAIL`, `CLI_SUBCOMMAND_ERROR`).
    /// `MCP_NO_OUTPUT` if the CLI emitted nothing,
    /// `MCP_MALFORMED_JSONL` if at least one line failed to
    /// parse, or a different `MCP_*` code from
    /// `evidence_core::HAND_EMITTED_MCP_CODES` on subprocess
    /// failure.
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
///
/// On success, `error = None`, `rules` is the manifest, `count`
/// equals `rules.len()`, and `exit_code` mirrors the CLI. On
/// tool-layer failure the response stays well-formed: `rules`
/// empty, `count == 0`, `exit_code == 2`, and `error` carries a
/// single `MCP_*` diagnostic — agents pattern-match on
/// `error.code` the same way they pattern-match on
/// `JsonlToolResponse.terminal` for the streaming tools.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct RulesToolResponse {
    /// Exit code advertised back to the host. `0` on successful
    /// pass-through; `2` on tool-layer failure (see `error`).
    pub exit_code: i32,

    /// The full rules manifest as emitted by the CLI — an array
    /// of `{code, severity, domain, has_fix_hint, terminal}`
    /// objects, alphabetically sorted by `code`. Empty on
    /// tool-layer failure.
    pub rules: Vec<serde_json::Value>,

    /// Convenience: `rules.len()`. Agents can pin this against
    /// `evidence_core::RULES.len()` for a drift check without
    /// deserializing every entry. `0` on tool-layer failure.
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

    /// Tool-layer failure diagnostic when the subprocess could
    /// not run or its stdout was not valid JSON. `None` on
    /// success. Carries an `MCP_*` code from
    /// `evidence_core::HAND_EMITTED_MCP_CODES` with
    /// `severity == "error"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<serde_json::Value>,
}

/// Empty-input marker for `evidence_ping`. `deny_unknown_fields`
/// matches the convention for the other MCP verb inputs — a
/// typo in the arguments object fails loud instead of running
/// silently. Required by HLR-054 / LLR-054.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PingRequest {}

/// Response shape for `evidence_ping` — a cheap liveness +
/// version-skew probe that does not spawn a subprocess.
///
/// `skew` is a short string tag rather than an enum variant
/// name so the JSON Schema is flat and agents pattern-match on
/// the string without serde-format coupling. Values are fixed
/// at the three `VersionSkew` outcomes: `"matched"`, `"skewed"`,
/// `"probe_failed"`.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PingResponse {
    /// evidence-mcp's `CARGO_PKG_VERSION` at build time.
    /// Always present.
    pub mcp_version: String,

    /// The cargo-evidence version captured by the one-shot
    /// startup probe. `Some(v)` on `"matched"` / `"skewed"`;
    /// `None` on `"probe_failed"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cli_version: Option<String>,

    /// Cached liveness state. One of `"matched"`, `"skewed"`,
    /// `"probe_failed"`. See [`crate::schema`] module doc for
    /// interpretation.
    pub skew: String,

    /// Populated only when `skew == "probe_failed"`, carrying
    /// the reason string captured at probe time (e.g.,
    /// `"cargo evidence --version spawn failed: no such file"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub probe_error: Option<String>,
}
