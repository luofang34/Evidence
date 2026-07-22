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
    Context,
    Coverage,
    Doctor,
    Env,
    Floors,
    Generate,
    Git,
    Hash,
    Init,
    Keygen,
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

mod constructors;
mod domain_map;
mod table;

pub use table::RULES;

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
