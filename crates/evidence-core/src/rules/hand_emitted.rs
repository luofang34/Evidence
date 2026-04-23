//! Hand-emitted-code registries. Split out of the parent
//! `rules.rs` facade to keep it under the workspace 500-line
//! file-size limit; pulled back in via `mod hand_emitted; pub use
//! hand_emitted::*;` from the parent.
//!
//! Two registries, one per emitting crate:
//!
//! - [`HAND_EMITTED_CLI_CODES`] — emitted from
//!   `crates/cargo-evidence/src`. Audited by
//!   `doctor_checks_locked::every_doctor_code_emitted_in_source`
//!   (and the parallel per-domain meta-checks).
//! - [`HAND_EMITTED_MCP_CODES`] — emitted from
//!   `crates/evidence-mcp/src`. Audited by
//!   `evidence-mcp/tests/mcp_codes_audit.rs`.
//!
//! Each set must stay disjoint from the other and from
//! [`crate::TERMINAL_CODES`]; the `rules::tests` module pins
//! those invariants.

/// Codes the CLI emits by hand that are NOT terminals. Pinned
/// here because `diagnostic_codes_locked` walks only `evidence-core/src`
/// for `DiagnosticCode` impls; hand-emitted CLI codes live in
/// `crates/cargo-evidence/src` and need an explicit registry.
///
/// Audited against `crates/cargo-evidence/src` by
/// `doctor_checks_locked::every_doctor_code_emitted_in_source`
/// (and parallel meta-checks per domain).
pub const HAND_EMITTED_CLI_CODES: &[&str] = &[
    "CHECK_TEST_RUNTIME_FAILURE",
    "CLI_INVALID_ARGUMENT",
    "CLI_UNSUPPORTED_FORMAT",
    "COVERAGE_BELOW_THRESHOLD",
    "COVERAGE_LLVMCOV_MISSING",
    "COVERAGE_OK",
    "COVERAGE_PARSE_FAILED",
    "DOCTOR_BOUNDARY_MISSING",
    "DOCTOR_CHECK_PASSED",
    "DOCTOR_CI_INTEGRATION_MISSING",
    "DOCTOR_FLOORS_BOUNDARY_MISMATCH",
    "DOCTOR_FLOORS_MISSING",
    "DOCTOR_FLOORS_SLACK",
    "DOCTOR_FLOORS_VIOLATED",
    "DOCTOR_MERGE_STYLE_RISK",
    "DOCTOR_MERGE_STYLE_UNKNOWN",
    "DOCTOR_OVERRIDE_PROTOCOL_UNDOCUMENTED",
    "DOCTOR_QUALIFICATION_MISSING",
    "DOCTOR_TRACE_EMPTY",
    "DOCTOR_TRACE_INVALID",
    "ENV_ENGINE_RELEASE_PROVENANCE",
    "FLOORS_BELOW_MIN",
    "FLOORS_DIMENSION_OK",
    "FLOORS_LOWERED_WITHOUT_JUSTIFICATION",
    "INIT_CERT_DIR_EXISTS",
    "INIT_TEMPLATE_WRITTEN",
    "TRACE_SELECTOR_UNRESOLVED",
    "VERIFY_BUNDLE_INCOMPLETE",
    "VERIFY_LLR_CHECK_SKIPPED_NO_OUTCOMES",
];

/// Codes the MCP layer (`crates/evidence-mcp`) emits by hand —
/// subprocess-wrapper failures (`RunError::code`) plus parse-layer
/// synthesized terminals. Parallel to [`HAND_EMITTED_CLI_CODES`];
/// kept disjoint so each set audits against its own source tree.
///
/// Audited against `crates/evidence-mcp/src` by
/// `evidence-mcp/tests/mcp_codes_audit.rs`.
pub const HAND_EMITTED_MCP_CODES: &[&str] = &[
    "MCP_CARGO_NOT_FOUND",
    "MCP_MALFORMED_JSONL",
    "MCP_NO_OUTPUT",
    "MCP_SUBPROCESS_SPAWN_FAILED",
    "MCP_SUBPROCESS_TIMEOUT",
    "MCP_VERSION_PROBE_FAILED",
    "MCP_VERSION_SKEW",
    "MCP_WORKSPACE_FALLBACK",
];

/// Codes in [`crate::RULES`] intentionally NOT claimed by any
/// LLR's `emits` list. `INIT_*` + `GENERATE_OK` + `GENERATE_FAIL`
/// ride the universal-JSONL surface; `cmd_init` and
/// `cmd_generate` don't yet have dedicated HLR/LLR chains — a
/// follow-up PR adds them and empties this list.
pub const RESERVED_UNCLAIMED_CODES: &[&str] = &[
    "GENERATE_FAIL",
    "GENERATE_OK",
    "INIT_CERT_DIR_EXISTS",
    "INIT_FAIL",
    "INIT_OK",
    "INIT_TEMPLATE_WRITTEN",
];
