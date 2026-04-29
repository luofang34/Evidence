//! Hand-curated manifest of every diagnostic code the tool can emit.
//! `RULES` is the single source of truth; exposed via
//! `cargo evidence rules --json`. Pinned by four bijection invariants
//! in `diagnostic_codes_locked`: RULES ⇔ DiagnosticCode::code(),
//! RULES ⇔ TERMINAL_CODES, RULES ⇔ HAND_EMITTED_CLI_CODES, and
//! ⋃(LLR.emits) ⇔ RULES.code. Entries sorted alphabetically by `code`.

use serde::Serialize;

use crate::diagnostic::Severity;

/// Top-level domain of a diagnostic code, derived from its prefix.
/// Variants correspond 1:1 to the code-prefix strings handled by
/// [`Domain::from_code`]. Variant names are self-documenting.
#[allow(missing_docs)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Domain {
    Boundary,
    Bundle,
    Check,
    Cli,
    Cmd,
    Coverage,
    Doctor,
    Env,
    Floors,
    Generate,
    Git,
    Hash,
    Init,
    Mcp,
    Policy,
    Req,
    Schema,
    Sign,
    Tests,
    Trace,
    Verify,
}

impl Domain {
    /// Derive a [`Domain`] from a code prefix. `None` for any
    /// unknown prefix — bijection test catches unmapped codes.
    /// Thin wrapper over `from_code_const` so the two stay in sync.
    pub fn from_code(code: &str) -> Option<Self> {
        Self::from_code_const(code)
    }
}

/// One row of the diagnostic manifest.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct RuleEntry {
    /// UPPER_SNAKE_CASE identifier (Schema Rule 3).
    pub code: &'static str,
    /// Reporter severity when the code is emitted.
    pub severity: Severity,
    /// Top-level domain, derived from prefix.
    pub domain: Domain,
    /// Whether the emit-site MAY populate `fix_hint`.
    pub has_fix_hint: bool,
    /// Hand-emitted terminal (Schema Rule 1). If true, also in
    /// [`TERMINAL_CODES`](crate::TERMINAL_CODES).
    pub terminal: bool,
}

mod hand_emitted;
pub use hand_emitted::{HAND_EMITTED_CLI_CODES, HAND_EMITTED_MCP_CODES, RESERVED_UNCLAIMED_CODES};

/// Hand-curated manifest of every emittable code. Sorted by `code`.
/// Additions: append, re-sort, claim in the relevant LLR's `emits`,
/// add a test exercising the emit.
pub const RULES: &[RuleEntry] = &[
    r(
        "BOUNDARY_CARGO_METADATA_FAILED",
        Severity::Error,
        Domain::Boundary,
    ),
    r(
        "BOUNDARY_CONFIG_PARSE_FAILED",
        Severity::Error,
        Domain::Boundary,
    ),
    r(
        "BOUNDARY_CONFIG_READ_FAILED",
        Severity::Error,
        Domain::Boundary,
    ),
    r(
        "BOUNDARY_FORBIDDEN_BUILD_RS",
        Severity::Error,
        Domain::Boundary,
    ),
    r(
        "BOUNDARY_FORBIDDEN_PROC_MACRO",
        Severity::Error,
        Domain::Boundary,
    ),
    r(
        "BOUNDARY_OUT_OF_SCOPE_DEPS",
        Severity::Error,
        Domain::Boundary,
    ),
    r(
        "BOUNDARY_PARSE_METADATA_FAILED",
        Severity::Error,
        Domain::Boundary,
    ),
    r(
        "BOUNDARY_UNKNOWN_IN_SCOPE_CRATE",
        Severity::Error,
        Domain::Boundary,
    ),
    r(
        "BOUNDARY_VERIFY_METADATA_MISSING",
        Severity::Error,
        Domain::Boundary,
    ),
    r("BUNDLE_ALREADY_EXISTS", Severity::Error, Domain::Bundle),
    r(
        "BUNDLE_CARGO_METADATA_FAILED",
        Severity::Error,
        Domain::Bundle,
    ),
    r("BUNDLE_CURRENT_DIR_FAILED", Severity::Error, Domain::Bundle),
    r("BUNDLE_DIRTY_GIT_TREE", Severity::Error, Domain::Bundle),
    r("BUNDLE_GIT_FAILED", Severity::Error, Domain::Bundle),
    r("BUNDLE_HASH_FAILED", Severity::Error, Domain::Bundle),
    r("BUNDLE_IO_FAILED", Severity::Error, Domain::Bundle),
    r("BUNDLE_PARSE_ENV_FAILED", Severity::Error, Domain::Bundle),
    r("BUNDLE_RUN_COMMAND_FAILED", Severity::Error, Domain::Bundle),
    r("BUNDLE_SERIALIZE_FAILED", Severity::Error, Domain::Bundle),
    r("BUNDLE_TOCTOU", Severity::Error, Domain::Bundle),
    r("CHECK_TEST_RUNTIME_FAILURE", Severity::Error, Domain::Check),
    cli("CLI_INVALID_ARGUMENT", Severity::Error),
    terminal("CLI_SUBCOMMAND_ERROR", Severity::Error),
    cli("CLI_UNSUPPORTED_FORMAT", Severity::Error),
    r("CMD_LAUNCH_FAILED", Severity::Error, Domain::Cmd),
    r("CMD_NON_UTF8_OUTPUT", Severity::Error, Domain::Cmd),
    r("CMD_NON_ZERO_EXIT", Severity::Error, Domain::Cmd),
    r(
        "COVERAGE_BELOW_THRESHOLD",
        Severity::Error,
        Domain::Coverage,
    ),
    r(
        "COVERAGE_LLVMCOV_MISSING",
        Severity::Error,
        Domain::Coverage,
    ),
    r("COVERAGE_OK", Severity::Info, Domain::Coverage),
    r("COVERAGE_PARSE_FAILED", Severity::Error, Domain::Coverage),
    r("DOCTOR_BOUNDARY_MISSING", Severity::Error, Domain::Doctor),
    r("DOCTOR_CHECK_PASSED", Severity::Info, Domain::Doctor),
    r(
        "DOCTOR_CI_INTEGRATION_MISSING",
        Severity::Warning,
        Domain::Doctor,
    ),
    terminal("DOCTOR_FAIL", Severity::Error),
    r(
        "DOCTOR_FLOORS_BOUNDARY_MISMATCH",
        Severity::Warning,
        Domain::Doctor,
    ),
    r("DOCTOR_FLOORS_MISSING", Severity::Error, Domain::Doctor),
    r("DOCTOR_FLOORS_SLACK", Severity::Warning, Domain::Doctor),
    r("DOCTOR_FLOORS_VIOLATED", Severity::Error, Domain::Doctor),
    r("DOCTOR_MERGE_STYLE_RISK", Severity::Warning, Domain::Doctor),
    r(
        "DOCTOR_MERGE_STYLE_UNKNOWN",
        Severity::Warning,
        Domain::Doctor,
    ),
    terminal("DOCTOR_OK", Severity::Info),
    r(
        "DOCTOR_OVERRIDE_PROTOCOL_UNDOCUMENTED",
        Severity::Warning,
        Domain::Doctor,
    ),
    r(
        "DOCTOR_QUALIFICATION_MISSING",
        Severity::Error,
        Domain::Doctor,
    ),
    r("DOCTOR_TRACE_EMPTY", Severity::Error, Domain::Doctor),
    r("DOCTOR_TRACE_INVALID", Severity::Error, Domain::Doctor),
    r(
        "ENV_ENGINE_RELEASE_PROVENANCE",
        Severity::Warning,
        Domain::Env,
    ),
    r("ENV_STRICT_CARGO_REQUIRED", Severity::Error, Domain::Env),
    r("ENV_STRICT_RUSTC_REQUIRED", Severity::Error, Domain::Env),
    floors("FLOORS_BELOW_MIN", Severity::Error),
    floors("FLOORS_DIMENSION_OK", Severity::Info),
    terminal("FLOORS_FAIL", Severity::Error),
    floors("FLOORS_LOWERED_WITHOUT_JUSTIFICATION", Severity::Error),
    terminal("FLOORS_OK", Severity::Info),
    terminal("GENERATE_FAIL", Severity::Error),
    terminal("GENERATE_OK", Severity::Info),
    r("GIT_CMD_FAILED", Severity::Error, Domain::Git),
    r("GIT_NON_UTF8_PATH", Severity::Error, Domain::Git),
    r("GIT_OTHER", Severity::Error, Domain::Git),
    r("GIT_SHALLOW_CLONE", Severity::Error, Domain::Git),
    r("GIT_STRICT_BRANCH_REQUIRED", Severity::Error, Domain::Git),
    r("GIT_STRICT_DIRTY_REQUIRED", Severity::Error, Domain::Git),
    r("GIT_STRICT_STATE_REQUIRED", Severity::Error, Domain::Git),
    r("GIT_SUBCOMMAND_FAILED", Severity::Error, Domain::Git),
    r("HASH_NON_UTF8_PATH", Severity::Error, Domain::Hash),
    r("HASH_NOT_UNDER_BASE", Severity::Error, Domain::Hash),
    r("HASH_OPEN_FAILED", Severity::Error, Domain::Hash),
    r("HASH_READ_FAILED", Severity::Error, Domain::Hash),
    r("HASH_WALK_FAILED", Severity::Error, Domain::Hash),
    r("HASH_WRITE_FAILED", Severity::Error, Domain::Hash),
    r("INIT_CERT_DIR_EXISTS", Severity::Error, Domain::Init),
    terminal("INIT_FAIL", Severity::Error),
    terminal("INIT_OK", Severity::Info),
    r("INIT_TEMPLATE_WRITTEN", Severity::Info, Domain::Init),
    r("MCP_CARGO_NOT_FOUND", Severity::Error, Domain::Mcp),
    r("MCP_MALFORMED_JSONL", Severity::Error, Domain::Mcp),
    r("MCP_NO_OUTPUT", Severity::Error, Domain::Mcp),
    r("MCP_SUBPROCESS_SPAWN_FAILED", Severity::Error, Domain::Mcp),
    r("MCP_SUBPROCESS_TIMEOUT", Severity::Error, Domain::Mcp),
    r("MCP_VERSION_PROBE_FAILED", Severity::Warning, Domain::Mcp),
    r("MCP_VERSION_SKEW", Severity::Warning, Domain::Mcp),
    r("MCP_WORKSPACE_FALLBACK", Severity::Warning, Domain::Mcp),
    r("POLICY_UNKNOWN_DAL", Severity::Error, Domain::Policy),
    r("POLICY_UNKNOWN_PROFILE", Severity::Error, Domain::Policy),
    req_gap("REQ_GAP"),
    req("REQ_PASS", Severity::Info),
    req("REQ_SKIP", Severity::Warning),
    r("SCHEMA_COMPILE_FAILED", Severity::Error, Domain::Schema),
    r("SCHEMA_INSTANCE_INVALID", Severity::Error, Domain::Schema),
    r("SCHEMA_PARSE_FAILED", Severity::Error, Domain::Schema),
    r("SIGN_INVALID_KEY", Severity::Error, Domain::Sign),
    r("SIGN_INVALID_SIGNATURE_HEX", Severity::Error, Domain::Sign),
    r("SIGN_READ_FAILED", Severity::Error, Domain::Sign),
    r("SIGN_WRITE_FAILED", Severity::Error, Domain::Sign),
    terminal("TESTS_OK", Severity::Info),
    r(
        "TESTS_OUTCOME_PARSE_FAILED",
        Severity::Warning,
        Domain::Tests,
    ),
    r("TRACE_BACKFILL_READ_FAILED", Severity::Error, Domain::Trace),
    r(
        "TRACE_BACKFILL_SERIALIZE_FAILED",
        Severity::Error,
        Domain::Trace,
    ),
    r(
        "TRACE_BACKFILL_WRITE_FAILED",
        Severity::Error,
        Domain::Trace,
    ),
    r(
        "TRACE_CONTRADICTORY_DERIVED",
        Severity::Error,
        Domain::Trace,
    ),
    r("TRACE_DANGLING_LINK", Severity::Error, Domain::Trace),
    r(
        "TRACE_DERIVED_MISSING_RATIONALE",
        Severity::Error,
        Domain::Trace,
    ),
    r("TRACE_DUPLICATE_TRACE_LINK", Severity::Error, Domain::Trace),
    r(
        "TRACE_HLR_SURFACE_UNCLAIMED",
        Severity::Error,
        Domain::Trace,
    ),
    r("TRACE_HLR_SURFACE_UNKNOWN", Severity::Error, Domain::Trace),
    r("TRACE_INVALID_LINK_UUID", Severity::Error, Domain::Trace),
    r("TRACE_LINK_FAILED", Severity::Error, Domain::Trace),
    r("TRACE_LINK_OTHER", Severity::Error, Domain::Trace),
    r(
        "TRACE_LLR_MISSING_PARENT_LINKS",
        Severity::Error,
        Domain::Trace,
    ),
    r(
        "TRACE_MISSING_HLR_SYS_TRACE",
        Severity::Error,
        Domain::Trace,
    ),
    r(
        "TRACE_MISSING_VERIFICATION_METHODS",
        Severity::Error,
        Domain::Trace,
    ),
    r("TRACE_OWNERSHIP_VIOLATION", Severity::Error, Domain::Trace),
    r("TRACE_PARSE_FAILED", Severity::Error, Domain::Trace),
    r("TRACE_READ_FAILED", Severity::Error, Domain::Trace),
    r("TRACE_REGISTER_FAILED", Severity::Error, Domain::Trace),
    r("TRACE_SELECTOR_UNRESOLVED", Severity::Error, Domain::Trace),
    r("TRACE_WRONG_TARGET_KIND", Severity::Error, Domain::Trace),
    r(
        "VERIFY_BOUNDARY_BUILD_RS_DETECTED",
        Severity::Error,
        Domain::Verify,
    ),
    r(
        "VERIFY_BOUNDARY_PROC_MACRO_DETECTED",
        Severity::Error,
        Domain::Verify,
    ),
    r(
        "VERIFY_BUNDLE_INCOMPLETE",
        Severity::Warning,
        Domain::Verify,
    ),
    r(
        "VERIFY_BUNDLE_INCOMPLETELY_CLAIMED",
        Severity::Error,
        Domain::Verify,
    ),
    r(
        "VERIFY_CONTENT_HASH_MISMATCH",
        Severity::Error,
        Domain::Verify,
    ),
    r(
        "VERIFY_CROSS_FILE_INCONSISTENCY",
        Severity::Error,
        Domain::Verify,
    ),
    r("VERIFY_DAL_MAP_MISMATCH", Severity::Error, Domain::Verify),
    r("VERIFY_DAL_MAP_ORPHAN", Severity::Error, Domain::Verify),
    r(
        "VERIFY_DETERMINISTIC_HASH_MISMATCH",
        Severity::Error,
        Domain::Verify,
    ),
    terminal("VERIFY_ERROR", Severity::Error),
    terminal("VERIFY_FAIL", Severity::Error),
    r("VERIFY_HASH_MISMATCH", Severity::Error, Domain::Verify),
    r("VERIFY_HMAC_FAILURE", Severity::Error, Domain::Verify),
    r("VERIFY_INVALID_FORMAT", Severity::Error, Domain::Verify),
    r(
        "VERIFY_LLR_CHECK_SKIPPED_NO_OUTCOMES",
        Severity::Info,
        Domain::Verify,
    ),
    r(
        "VERIFY_LLR_TEST_SELECTOR_UNRESOLVED",
        Severity::Error,
        Domain::Verify,
    ),
    r(
        "VERIFY_MANIFEST_PROJECTION_DRIFT",
        Severity::Error,
        Domain::Verify,
    ),
    r(
        "VERIFY_MISSING_HASHED_FILE",
        Severity::Error,
        Domain::Verify,
    ),
    terminal("VERIFY_OK", Severity::Info),
    r("VERIFY_PRERELEASE_TOOL", Severity::Error, Domain::Verify),
    r(
        "VERIFY_RUNTIME_BUNDLE_NOT_FOUND",
        Severity::Error,
        Domain::Verify,
    ),
    r("VERIFY_RUNTIME_HASH", Severity::Error, Domain::Verify),
    r(
        "VERIFY_RUNTIME_PARSE_INDEX",
        Severity::Error,
        Domain::Verify,
    ),
    r("VERIFY_RUNTIME_READ_FILE", Severity::Error, Domain::Verify),
    r(
        "VERIFY_RUNTIME_READ_VERIFY_KEY",
        Severity::Error,
        Domain::Verify,
    ),
    r("VERIFY_RUNTIME_SIGNING", Severity::Error, Domain::Verify),
    r("VERIFY_RUNTIME_WALK", Severity::Error, Domain::Verify),
    r(
        "VERIFY_TEST_SUMMARY_ABSENT_ON_FAILED_RUN",
        Severity::Error,
        Domain::Verify,
    ),
    r(
        "VERIFY_TEST_SUMMARY_MISMATCH",
        Severity::Error,
        Domain::Verify,
    ),
    r(
        "VERIFY_TOOL_COMMANDS_FAILED_SILENTLY",
        Severity::Error,
        Domain::Verify,
    ),
    r(
        "VERIFY_TRACE_OUTPUT_NOT_HASHED",
        Severity::Error,
        Domain::Verify,
    ),
    r("VERIFY_UNEXPECTED_FILE", Severity::Error, Domain::Verify),
    r("VERIFY_UNSAFE_PATH", Severity::Error, Domain::Verify),
];

// Constructor helpers — kept `const fn` so RULES stays a true const.

const fn r(code: &'static str, severity: Severity, domain: Domain) -> RuleEntry {
    RuleEntry {
        code,
        severity,
        domain,
        has_fix_hint: false,
        terminal: false,
    }
}

const fn req(code: &'static str, severity: Severity) -> RuleEntry {
    RuleEntry {
        code,
        severity,
        domain: Domain::Req,
        has_fix_hint: false,
        terminal: false,
    }
}

const fn req_gap(code: &'static str) -> RuleEntry {
    RuleEntry {
        code,
        severity: Severity::Error,
        domain: Domain::Req,
        has_fix_hint: true,
        terminal: false,
    }
}

const fn cli(code: &'static str, severity: Severity) -> RuleEntry {
    RuleEntry {
        code,
        severity,
        domain: Domain::Cli,
        has_fix_hint: false,
        terminal: false,
    }
}

const fn floors(code: &'static str, severity: Severity) -> RuleEntry {
    RuleEntry {
        code,
        severity,
        domain: Domain::Floors,
        has_fix_hint: false,
        terminal: false,
    }
}

const fn terminal(code: &'static str, severity: Severity) -> RuleEntry {
    RuleEntry {
        code,
        severity,
        domain: match Domain::from_code_const(code) {
            Some(d) => d,
            None => Domain::Cli,
        },
        has_fix_hint: false,
        terminal: true,
    }
}

mod domain_map;

/// Serialize [`RULES`] as a JSON array for `cargo evidence rules
/// --json`. Deterministic (alphabetical by `code`).
pub fn rules_json() -> String {
    #[allow(
        clippy::expect_used,
        reason = "RULES is a const with infallibly-serializable field types"
    )]
    {
        serde_json::to_string(RULES).expect("RULES is statically serializable")
    }
}

// Tests live in a sibling file pulled in via `#[path]` so this
// facade stays under the 500-line workspace limit.
#[cfg(test)]
#[path = "rules/tests.rs"]
mod tests;
