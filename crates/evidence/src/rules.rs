//! Hand-curated manifest of every diagnostic code the tool can emit.
//!
//! `RULES` is the single source of truth for "what can the tool say?"
//! — exposed to agents via `cargo evidence rules --json` and pinned
//! by four bijection invariants in `diagnostic_codes_locked`:
//!
//! 1. `RULES ⇔ source DiagnosticCode::code() returns` (library walk).
//! 2. `RULES ⇔ TERMINAL_CODES` (for entries flagged `terminal = true`).
//! 3. `RULES ⇔ HAND_EMITTED_CLI_CODES` (for non-terminal CLI emits).
//! 4. `⋃(LLR.emits) ⇔ RULES.code` (the code↔requirement loop).
//!
//! The const is deliberately hand-authored: adding a new code forces a
//! reviewer-visible edit here, and a missing edit fires a specific,
//! targeted CI failure. Auto-generating `RULES` from source would
//! remove that friction and defeat the whole point of PR #47.
//!
//! **Ordering.** Entries are sorted alphabetically by `code` so
//! `rules_json()` output is deterministic. `diagnostic_codes_locked`
//! asserts sort order; a hand-inserted out-of-order entry fails CI.
//!
//! **Per-code `has_fix_hint`.** A `true` value means "this code CAN
//! carry a FixHint in at least one emit site" — an audit-trail label,
//! not a runtime contract. Today only `REQ_GAP` (PR #46) carries
//! mechanical FixHints. Future widenings (PR #50's MCP autofix)
//! extend this.

use serde::Serialize;

use crate::diagnostic::Severity;

/// Top-level domain of a diagnostic code, derived from its prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Domain {
    /// `BOUNDARY_*` — workspace-scope / `cert/boundary.toml`.
    Boundary,
    /// `BUNDLE_*` — bundle lifecycle (generate).
    Bundle,
    /// `CLI_*` — CLI-layer hand-emitted codes (not from the library).
    Cli,
    /// `CMD_*` — subprocess execution.
    Cmd,
    /// `ENV_*` — environment probe (rustc / cargo / toolchain).
    Env,
    /// `FLOORS_*` — ratcheting-floors gate (PR #48).
    Floors,
    /// `GIT_*` — git snapshot.
    Git,
    /// `HASH_*` — content-hashing subsystem.
    Hash,
    /// `POLICY_*` — DAL / profile parsing.
    Policy,
    /// `REQ_*` — per-requirement pass/gap/skip (PR #46).
    Req,
    /// `SCHEMA_*` — JSON Schema validation.
    Schema,
    /// `SIGN_*` — signing / HMAC key IO.
    Sign,
    /// `TRACE_*` — trace-file validation.
    Trace,
    /// `VERIFY_*` — bundle verification.
    Verify,
}

impl Domain {
    /// Derive a [`Domain`] from a code prefix. Returns `None` for any
    /// code whose prefix doesn't match a known domain — the
    /// bijection test asserts every RULES entry's prefix matches its
    /// declared domain, so an unmapped prefix fires a targeted
    /// failure.
    pub fn from_code(code: &str) -> Option<Self> {
        let prefix = code.split('_').next().unwrap_or(code);
        Some(match prefix {
            "BOUNDARY" => Self::Boundary,
            "BUNDLE" => Self::Bundle,
            "CLI" => Self::Cli,
            "CMD" => Self::Cmd,
            "ENV" => Self::Env,
            "FLOORS" => Self::Floors,
            "GIT" => Self::Git,
            "HASH" => Self::Hash,
            "POLICY" => Self::Policy,
            "REQ" => Self::Req,
            "SCHEMA" => Self::Schema,
            "SIGN" => Self::Sign,
            "TRACE" => Self::Trace,
            "VERIFY" => Self::Verify,
            _ => return None,
        })
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
    /// Whether the emit-site MAY populate `fix_hint` for this code.
    /// Audit label only — runtime populate is per-emit-site logic.
    pub has_fix_hint: bool,
    /// Whether this code is a hand-emitted terminal (Schema Rule 1).
    /// If true, the code also appears in
    /// [`TERMINAL_CODES`](crate::TERMINAL_CODES) and ends in
    /// `_OK` / `_FAIL` / `_ERROR`.
    pub terminal: bool,
}

/// Codes the CLI emits by hand (as `Diagnostic { code: "…".into(), … }`)
/// that are NOT terminals. Library-side impls don't return these —
/// they live in `cargo-evidence`'s CLI modules. Pinned here because
/// `diagnostic_codes_locked` walks only `crates/evidence/src`, so CLI
/// emits need an explicit bless list for the bijection.
///
/// Terminals (`VERIFY_OK`, `VERIFY_FAIL`, `VERIFY_ERROR`,
/// `CLI_SUBCOMMAND_ERROR`) live in
/// [`TERMINAL_CODES`](crate::TERMINAL_CODES) — keep these two lists
/// disjoint.
pub const HAND_EMITTED_CLI_CODES: &[&str] = &[
    "CLI_INVALID_ARGUMENT",
    "CLI_UNSUPPORTED_FORMAT",
    "FLOORS_BELOW_MIN",
    "FLOORS_LOWERED_WITHOUT_JUSTIFICATION",
    "TRACE_SELECTOR_UNRESOLVED",
];

/// Codes declared in `RULES` that are intentionally NOT claimed by any
/// LLR's `emits` list. Must stay empty or be justified here in
/// writing: the bijection test uses this set as its only allowed
/// gap between `RULES` and `⋃(LLR.emits)`.
pub const RESERVED_UNCLAIMED_CODES: &[&str] = &[];

/// Hand-curated manifest of every code the tool can emit. Sorted
/// alphabetically by `code` for deterministic serialization.
///
/// Additions: append the entry, re-sort, update the LLR in
/// `tool/trace/llr.toml` whose `emits` list owns this code (or add
/// one), and add a test exercising the emit path.
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
    r("BUNDLE_ALREADY_EXISTS", Severity::Error, Domain::Bundle),
    r("BUNDLE_CURRENT_DIR_FAILED", Severity::Error, Domain::Bundle),
    r("BUNDLE_DIRTY_GIT_TREE", Severity::Error, Domain::Bundle),
    r("BUNDLE_GIT_FAILED", Severity::Error, Domain::Bundle),
    r("BUNDLE_HASH_FAILED", Severity::Error, Domain::Bundle),
    r("BUNDLE_IO_FAILED", Severity::Error, Domain::Bundle),
    r("BUNDLE_PARSE_ENV_FAILED", Severity::Error, Domain::Bundle),
    r("BUNDLE_RUN_COMMAND_FAILED", Severity::Error, Domain::Bundle),
    r("BUNDLE_SERIALIZE_FAILED", Severity::Error, Domain::Bundle),
    r("BUNDLE_TOCTOU", Severity::Error, Domain::Bundle),
    cli("CLI_INVALID_ARGUMENT", Severity::Error),
    terminal("CLI_SUBCOMMAND_ERROR", Severity::Error),
    cli("CLI_UNSUPPORTED_FORMAT", Severity::Error),
    r("CMD_LAUNCH_FAILED", Severity::Error, Domain::Cmd),
    r("CMD_NON_UTF8_OUTPUT", Severity::Error, Domain::Cmd),
    r("CMD_NON_ZERO_EXIT", Severity::Error, Domain::Cmd),
    r("ENV_STRICT_CARGO_REQUIRED", Severity::Error, Domain::Env),
    r("ENV_STRICT_RUSTC_REQUIRED", Severity::Error, Domain::Env),
    floors("FLOORS_BELOW_MIN", Severity::Error),
    floors("FLOORS_LOWERED_WITHOUT_JUSTIFICATION", Severity::Error),
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
    r("VERIFY_RUNTIME_SIGNING", Severity::Error, Domain::Verify),
    r("VERIFY_RUNTIME_WALK", Severity::Error, Domain::Verify),
    r(
        "VERIFY_TEST_SUMMARY_MISMATCH",
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

impl Domain {
    /// `const fn` twin of [`Domain::from_code`] used inside the
    /// `terminal(…)` constructor.
    const fn from_code_const(code: &str) -> Option<Self> {
        // Prefix is the segment before the first underscore.
        let bytes = code.as_bytes();
        let mut i = 0;
        while i < bytes.len() && bytes[i] != b'_' {
            i += 1;
        }
        let prefix = match std::str::from_utf8(bytes.split_at(i).0) {
            Ok(s) => s,
            Err(_) => return None,
        };
        // `str::eq` isn't const, so compare byte-wise.
        match prefix.as_bytes() {
            b"BOUNDARY" => Some(Self::Boundary),
            b"BUNDLE" => Some(Self::Bundle),
            b"CLI" => Some(Self::Cli),
            b"CMD" => Some(Self::Cmd),
            b"ENV" => Some(Self::Env),
            b"FLOORS" => Some(Self::Floors),
            b"GIT" => Some(Self::Git),
            b"HASH" => Some(Self::Hash),
            b"POLICY" => Some(Self::Policy),
            b"REQ" => Some(Self::Req),
            b"SCHEMA" => Some(Self::Schema),
            b"SIGN" => Some(Self::Sign),
            b"TRACE" => Some(Self::Trace),
            b"VERIFY" => Some(Self::Verify),
            _ => None,
        }
    }
}

/// Serialize [`RULES`] as a JSON array suitable for agents consuming
/// `cargo evidence rules --json`. Output is deterministic (alphabetical
/// by `code`, matching the const's committed order).
pub fn rules_json() -> String {
    // `RULES` is compile-time; every field serializes infallibly. The
    // `allow` documents that the only failure mode is impossible.
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
