//! Agent-consumable diagnostic format.
//!
//! Every typed error enum in the library eventually becomes one of
//! these on the wire so an AI agent (or any structured-output consumer)
//! can pattern-match on a stable `code` rather than parse prose.
//!
//! # Contract (10 Schema Rules, load-bearing — see `schemas/diagnostic.schema.json`)
//!
//! 1. **Exit code ↔ terminal event.** Every run emits exactly one
//!    terminal event as the last JSONL line on stdout. `exit 0` ⟺
//!    terminal ends in `_OK`. `exit 2` ⟺ terminal ends in `_FAIL`.
//!    `exit 1` ⟺ terminal ends in `_ERROR` (runtime fault — the
//!    finding diag is emitted first, followed by the terminal so
//!    readers can detect stream truncation). Suffixes `_OK`, `_FAIL`,
//!    `_ERROR` are terminal-only — non-terminal findings MUST NOT end
//!    in these suffixes. [`TERMINAL_CODES`] is the single source of
//!    truth for hand-emitted terminals, locked by the
//!    `diagnostic_codes_locked` test.
//! 2. **stdout strict.** In `--format=jsonl` mode, stdout is only
//!    JSONL; human progress text stays on stderr.
//! 3. **Codes locked by test.** Uniqueness + regex; exhaustive `match`
//!    in [`DiagnosticCode::code`] impls is the real enforcement.
//! 4. **Flush per event.** The emit helper flushes after every line —
//!    block-buffered stdout on pipes would stall streaming agents.
//! 5. **`--json` is a permanent alias** for `--format=json`; no
//!    deprecation.
//! 6. **[`FixHint`] is forward-compatible** via
//!    `#[serde(tag = "kind", other)]`. Agents MUST tolerate unknown
//!    `kind` (deserializes as [`FixHint::Other`]).
//! 7. **Events are independent observations.** A single root cause may
//!    produce multiple downstream events; agents should dedupe by
//!    severity-ranked code before proposing fixes.
//! 8. **[`Location`] precedence.** Agents SHOULD prefer `toml_path`
//!    (structural, stable) over `entry_uid` (semantic, renameable);
//!    `file`/`line`/`col` are the last fallback.
//! 9. **`schema show diagnostic`** prints this wire-format schema;
//!    `schema validate` deliberately doesn't recognize diagnostic
//!    files (they're streamed, not committed).
//! 10. **[`Severity`] is a closed enum.** Unknown values fail
//!     deserialization. Opposite policy from [`FixHint`] because
//!     severity drives exit-code decisions.
//!
//! # Usage
//!
//! Library error enums implement [`DiagnosticCode`]:
//!
//! ```ignore
//! impl DiagnosticCode for MyError {
//!     fn code(&self) -> &'static str {
//!         match self {
//!             MyError::OutOfBudget => "MY_OUT_OF_BUDGET",
//!             MyError::BadChecksum => "MY_BAD_CHECKSUM",
//!         }
//!     }
//!     fn severity(&self) -> Severity { Severity::Error }
//! }
//! ```
//!
//! The CLI converts via [`DiagnosticCode::to_diagnostic`] and streams
//! through the `emit_jsonl` helper in `cargo-evidence/src/cli/output.rs`.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Hand-emitted terminal codes — the single source of truth across the
/// whole tool for codes that end a `--format=jsonl` stream.
///
/// Every entry here:
///
/// 1. Ends in one of the reserved terminal suffixes (`_OK`, `_FAIL`,
///    `_ERROR`) per Schema Rule 1.
/// 2. Is emitted directly by the CLI layer (never returned from any
///    [`DiagnosticCode::code`] impl). That disjointness is asserted by
///    the `diagnostic_codes_locked` integration test.
///
/// Adding a new hand-emitted terminal means: (a) append it here, (b)
/// make sure it ends in a reserved suffix, (c) confirm the locked-codes
/// test still passes (it will enforce the two invariants above).
pub const TERMINAL_CODES: &[&str] = &[
    "VERIFY_OK",
    "VERIFY_FAIL",
    "VERIFY_ERROR",
    "CLI_SUBCOMMAND_ERROR",
];

/// One observation in the diagnostic stream.
///
/// Wire shape: see `schemas/diagnostic.schema.json`. Adding an optional
/// field (like [`subcommand`](Self::subcommand)), a new [`FixHint`]
/// variant, or extra fields on [`Location`] is backwards-compatible per
/// Schema Rules 6 and 8. Changing an existing field's type or removing
/// a required field is a contract break.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Diagnostic {
    /// Stable UPPER_SNAKE_CASE identifier, domain-prefixed
    /// (`VERIFY_*` / `BUNDLE_*` / `TRACE_*` / `BOUNDARY_*` / `CLI_*` / …).
    ///
    /// Matched by agents; never re-interpreted from `message`.
    pub code: String,

    /// Severity drives exit-code logic; see Schema Rule 1.
    pub severity: Severity,

    /// Human-readable one-liner. Stable across patch releases but not
    /// a match target — agents should key on `code`, not `message`.
    pub message: String,

    /// Optional structural pointer to where the issue lives. Agents
    /// preferring autofix will key on `location.toml_path` when
    /// present (Schema Rule 8).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<Location>,

    /// Optional hint a fixer tool can act on. Forward-compatible:
    /// unknown `kind` deserializes as [`FixHint::Other`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix_hint: Option<FixHint>,

    /// Optional cargo-evidence subcommand name. Populated on terminals
    /// that span subcommands — currently only [`CLI_SUBCOMMAND_ERROR`]
    /// (the terminal emitted when `--format=jsonl` is used against a
    /// subcommand that doesn't yet stream JSONL natively). Absent on
    /// every other diagnostic.
    ///
    /// [`CLI_SUBCOMMAND_ERROR`]: TERMINAL_CODES
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subcommand: Option<String>,
}

/// Closed severity enum — see Schema Rule 10. An unknown variant
/// during deserialization MUST fail the parse; silent tolerance would
/// let bad bundles slip past the exit-code contract.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// Fatal finding — exit code 2 (verification) or 1 (runtime).
    Error,
    /// Non-fatal finding — advisory; exit code still 0 unless
    /// `--strict` elevates.
    Warning,
    /// Progress or metadata event — never changes exit code.
    Info,
}

/// Structural pointer attached to a diagnostic.
///
/// Fields are independently optional so each emitter attaches only
/// what it has. `file`/`line`/`col` describe source-text positions
/// (e.g. `toml::de::Error::span()`); `toml_path` is a JSON-pointer-
/// style structural locator like `requirements[2].uid`; `entry_uid`
/// is a semantic fallback used when the UUID is the stable handle
/// an agent should reference in a fix.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Location {
    /// Workspace-relative file path when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<PathBuf>,

    /// 1-based line number into `file`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,

    /// 1-based column number into `file`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub col: Option<u32>,

    /// Structural locator, e.g. `requirements[2].uid`. Agent-preferred
    /// per Schema Rule 8.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub toml_path: Option<String>,

    /// Semantic UUID fallback. Useful when the containing file has
    /// been reshuffled; stable across renames of the human-readable
    /// `id` field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry_uid: Option<String>,
}

/// Machine-readable hint for an automated fixer.
///
/// This PR populates the variants sparsely — it's the wire shape that
/// matters. PR #5 (`cargo evidence fix --dry-run`) will widen the
/// variants; existing agents won't break because [`FixHint::Other`] is
/// the catch-all per Schema Rule 6.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FixHint {
    /// Generate a UUID for an entry missing one. Toml path points to
    /// the entry (not the `uid` field itself).
    AssignUuid {
        /// File to edit.
        path: PathBuf,
        /// Structural locator for the target entry.
        toml_path: String,
    },

    /// Add a missing TOML key at the given structural path.
    AddTomlKey {
        /// File to edit.
        path: PathBuf,
        /// Structural locator for the target table.
        toml_path: String,
        /// Key to add.
        key: String,
        /// Placeholder value the human should replace.
        value_stub: String,
    },

    /// Catch-all for variants introduced in future schema versions.
    /// Deserialized when the `kind` tag doesn't match any known
    /// variant; agents MUST tolerate this per Schema Rule 6.
    #[serde(other)]
    Other,
}

/// Every typed error in the library implements this trait so the CLI
/// can stream it as a [`Diagnostic`] without re-encoding prose.
///
/// Implementers MUST use `match self` on the error enum in [`code`]
/// so Rust's exhaustiveness catches a new variant without a stable
/// code at compile time. The locked-codes contract test only enforces
/// uniqueness + regex; reference-check is delegated to the compiler.
///
/// [`code`]: DiagnosticCode::code
pub trait DiagnosticCode: std::fmt::Display {
    /// Stable UPPER_SNAKE_CASE identifier.
    fn code(&self) -> &'static str;

    /// Severity; almost always [`Severity::Error`] for typed-error
    /// enums, but trait-default-free to let terminal events (e.g.
    /// `VERIFY_OK`) downgrade.
    fn severity(&self) -> Severity;

    /// Optional structural pointer. Override when the error carries
    /// enough context (e.g. a file path, a TOML span, an entry UID).
    fn location(&self) -> Option<Location> {
        None
    }

    /// Optional autofix hint. Override when a mechanical fix exists
    /// (e.g. `TRACE_UID_MISSING` → [`FixHint::AssignUuid`]). PR #5
    /// will populate most of these.
    fn fix_hint(&self) -> Option<FixHint> {
        None
    }

    /// Derive a [`Diagnostic`] by combining [`code`](Self::code),
    /// [`severity`](Self::severity), [`Display`](std::fmt::Display),
    /// [`location`](Self::location), [`fix_hint`](Self::fix_hint).
    ///
    /// The `subcommand` field is always `None` on trait-derived
    /// diagnostics — it's reserved for CLI-layer terminals
    /// (e.g. `CLI_SUBCOMMAND_ERROR`) that the library never emits.
    fn to_diagnostic(&self) -> Diagnostic {
        Diagnostic {
            code: self.code().to_string(),
            severity: self.severity(),
            message: self.to_string(),
            location: self.location(),
            fix_hint: self.fix_hint(),
            subcommand: None,
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
mod tests {
    use super::*;

    /// Schema Rule 10: unknown `Severity` values must fail
    /// deserialization. Opposite policy from [`FixHint`] because
    /// severity drives exit-code decisions.
    #[test]
    fn severity_rejects_unknown_variant() {
        let result: Result<Severity, _> = serde_json::from_str(r#""hint""#);
        assert!(
            result.is_err(),
            "unknown Severity value must fail deserialization (Schema Rule 10); got {:?}",
            result
        );
    }

    /// Schema Rule 10: the three known variants round-trip.
    #[test]
    fn severity_round_trip_known_variants() {
        for sev in [Severity::Error, Severity::Warning, Severity::Info] {
            let s = serde_json::to_string(&sev).unwrap();
            let back: Severity = serde_json::from_str(&s).unwrap();
            assert_eq!(sev, back);
        }
    }

    /// Schema Rule 6: a JSON payload with an unknown `kind` must
    /// deserialize as [`FixHint::Other`] rather than fail, so PR #5
    /// can widen the enum without breaking existing agents.
    #[test]
    fn fix_hint_unknown_kind_falls_back_to_other() {
        let payload = r#"{"kind": "patch_requirement_text", "path": "x", "value": 42}"#;
        let hint: FixHint = serde_json::from_str(payload).unwrap();
        assert_eq!(hint, FixHint::Other);
    }

    /// Known FixHint variants round-trip through serde.
    #[test]
    fn fix_hint_assign_uuid_round_trips() {
        let hint = FixHint::AssignUuid {
            path: PathBuf::from("cert/trace/llr.toml"),
            toml_path: "requirements[3]".to_string(),
        };
        let s = serde_json::to_string(&hint).unwrap();
        let back: FixHint = serde_json::from_str(&s).unwrap();
        assert_eq!(hint, back);
    }

    /// Known FixHint variants tag on `kind` with `snake_case`.
    #[test]
    fn fix_hint_serializes_with_kind_tag() {
        let hint = FixHint::AddTomlKey {
            path: PathBuf::from("a.toml"),
            toml_path: "section.subsection".to_string(),
            key: "rationale".to_string(),
            value_stub: "<fill in>".to_string(),
        };
        let s = serde_json::to_string(&hint).unwrap();
        assert!(s.contains(r#""kind":"add_toml_key""#), "got {}", s);
        assert!(s.contains(r#""key":"rationale""#));
    }

    /// `Diagnostic` with only required fields omits optional ones
    /// rather than emitting `null`. Keeps JSONL lines compact.
    #[test]
    fn diagnostic_skips_optional_fields_when_none() {
        let d = Diagnostic {
            code: "TEST_CODE".to_string(),
            severity: Severity::Info,
            message: "hi".to_string(),
            location: None,
            fix_hint: None,
            subcommand: None,
        };
        let s = serde_json::to_string(&d).unwrap();
        assert!(!s.contains("location"), "got {}", s);
        assert!(!s.contains("fix_hint"), "got {}", s);
        assert!(!s.contains("subcommand"), "got {}", s);
    }

    /// `Diagnostic` round-trips with all fields populated.
    #[test]
    fn diagnostic_round_trip_with_location_and_fix_hint() {
        let d = Diagnostic {
            code: "TRACE_UID_MISSING".to_string(),
            severity: Severity::Error,
            message: "HLR-001 missing UID".to_string(),
            location: Some(Location {
                file: Some(PathBuf::from("cert/trace/hlr.toml")),
                line: Some(12),
                col: Some(3),
                toml_path: Some("requirements[0]".to_string()),
                entry_uid: None,
            }),
            fix_hint: Some(FixHint::AssignUuid {
                path: PathBuf::from("cert/trace/hlr.toml"),
                toml_path: "requirements[0]".to_string(),
            }),
            subcommand: None,
        };
        let s = serde_json::to_string(&d).unwrap();
        let back: Diagnostic = serde_json::from_str(&s).unwrap();
        assert_eq!(d, back);
    }

    /// `CLI_SUBCOMMAND_ERROR` terminal carries the subcommand name in
    /// the `subcommand` field; the round-trip preserves it verbatim.
    #[test]
    fn diagnostic_subcommand_field_round_trips_when_set() {
        let d = Diagnostic {
            code: "CLI_SUBCOMMAND_ERROR".to_string(),
            severity: Severity::Error,
            message: "subcommand 'generate' does not support --format=jsonl".to_string(),
            location: None,
            fix_hint: None,
            subcommand: Some("generate".to_string()),
        };
        let s = serde_json::to_string(&d).unwrap();
        assert!(s.contains(r#""subcommand":"generate""#), "got {}", s);
        let back: Diagnostic = serde_json::from_str(&s).unwrap();
        assert_eq!(d, back);
    }

    /// [`TERMINAL_CODES`] invariants: every entry ends in a reserved
    /// terminal suffix (`_OK` / `_FAIL` / `_ERROR`), and the slice has
    /// no duplicates. The `diagnostic_codes_locked` integration test
    /// re-checks both globally — this test pins the invariant at the
    /// library crate's unit level so a local dev edit catches the
    /// problem before integration-test time.
    #[test]
    fn terminal_codes_all_end_in_reserved_suffix() {
        for code in TERMINAL_CODES {
            assert!(
                code.ends_with("_OK") || code.ends_with("_FAIL") || code.ends_with("_ERROR"),
                "TERMINAL_CODES entry '{}' does not end in a reserved suffix",
                code,
            );
        }
        let mut seen = std::collections::BTreeSet::new();
        for code in TERMINAL_CODES {
            assert!(
                seen.insert(*code),
                "TERMINAL_CODES contains duplicate '{}'",
                code
            );
        }
    }

    /// `Location` is always self-consistent: an all-None Location is
    /// legal and serializes to `{}`. Emitters use that shape when no
    /// concrete positional information is available.
    #[test]
    fn location_default_is_empty_object() {
        let loc = Location::default();
        let s = serde_json::to_string(&loc).unwrap();
        assert_eq!(s, "{}");
    }

    /// The trait's `to_diagnostic` default impl wires [`code`],
    /// [`severity`], [`Display`], [`location`], [`fix_hint`].
    #[test]
    fn to_diagnostic_default_impl_wires_every_field() {
        struct Fake;
        impl std::fmt::Display for Fake {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "fake message")
            }
        }
        impl DiagnosticCode for Fake {
            fn code(&self) -> &'static str {
                "FAKE_TEST_CODE"
            }
            fn severity(&self) -> Severity {
                Severity::Warning
            }
            fn location(&self) -> Option<Location> {
                Some(Location {
                    file: Some(PathBuf::from("a.rs")),
                    ..Location::default()
                })
            }
        }

        let d = Fake.to_diagnostic();
        assert_eq!(d.code, "FAKE_TEST_CODE");
        assert_eq!(d.severity, Severity::Warning);
        assert_eq!(d.message, "fake message");
        assert_eq!(d.location.unwrap().file, Some(PathBuf::from("a.rs")));
        assert!(d.fix_hint.is_none());
    }
}
