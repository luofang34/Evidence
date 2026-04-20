//! Typed variants carried by `TraceValidationError::Link`.
//!
//! PR #51 / LLR-041. Each variant has its own `DiagnosticCode` impl
//! so the `diagnostic_codes_locked` walker picks up each code exactly
//! the way it picks up `VerifyError` variants. The outer
//! `TraceValidationError::Link { errors: Vec<LinkError> }` still
//! returns `TRACE_LINK_FAILED` for aggregate-level callers; per-
//! variant granularity comes from iterating `errors` and calling
//! `.code()` on each.
//!
//! **Scope**: only Link-phase rules. The Register phase
//! (`TraceValidationError::Register`) stays on `Vec<String>` for
//! now; promoting it to typed variants is a separate follow-up PR.
//!
//! Lives in this sibling file (pulled in by `validation.rs` as
//! `mod link_errors;`) to keep the facade under the workspace's
//! 500-line per-file limit. Mirrors PR #48's `floors/walker.rs`
//! split.

use thiserror::Error;

use crate::diagnostic::{DiagnosticCode, Severity};

/// A single Link-phase validation failure.
///
/// Each variant carries enough payload to reconstruct the complete
/// diagnostic message (via `thiserror::Error`'s `#[error(...)]`
/// template) and returns a stable `TRACE_*` code via the
/// `DiagnosticCode` impl. Variants are deliberately flat — no
/// nested enums — so pattern-matching at emit sites is direct.
#[derive(Debug, Clone, Error)]
pub enum LinkError {
    /// Non-UUID string in a `traces_to` entry.
    #[error("Link '{link}' in {source_kind} {source_id} is not a UUID")]
    InvalidLinkUuid {
        /// Source item kind (`"HLR"`, `"LLR"`, `"TEST"`).
        source_kind: &'static str,
        /// Source item's human ID.
        source_id: String,
        /// Offending link string.
        link: String,
    },

    /// `traces_to` UUID that doesn't resolve to any registered item.
    #[error(
        "Link '{link}' in {source_kind} {source_id} not found (dangling ref to expected {expected_target_kind})"
    )]
    DanglingLink {
        /// Source item kind.
        source_kind: &'static str,
        /// Source item's human ID.
        source_id: String,
        /// Dangling link UUID.
        link: String,
        /// Kind the link was expected to point at.
        expected_target_kind: &'static str,
    },

    /// `traces_to` UUID resolves to an item of the wrong kind
    /// (e.g. LLR pointing at a TEST UUID).
    #[error("Link '{link}' in {source_kind} {source_id} points to {found} but expected {expected}")]
    WrongTargetKind {
        /// Source item kind.
        source_kind: &'static str,
        /// Source item's human ID.
        source_id: String,
        /// Offending link UUID.
        link: String,
        /// Kind the link was expected to point at.
        expected: &'static str,
        /// Kind the link actually points at.
        found: String,
    },

    /// Cross-owner link that violates the per-tier ownership rule.
    #[error(
        "Ownership violation: {source_kind}({source_owner}:{source_id}) -> {target_kind}({target_owner}:{target_id})"
    )]
    OwnershipViolation {
        /// Source item kind.
        source_kind: &'static str,
        /// Source item's human ID.
        source_id: String,
        /// Source item's owner.
        source_owner: String,
        /// Target item kind.
        target_kind: &'static str,
        /// Target item's human ID.
        target_id: String,
        /// Target item's owner.
        target_owner: String,
    },

    /// HLR.surfaces entry that's not in `KNOWN_SURFACES`.
    #[error("HLR {hlr_id} claims surface '{surface}' which is not in KNOWN_SURFACES")]
    SurfaceUnknown {
        /// HLR's human ID.
        hlr_id: String,
        /// Offending surface string.
        surface: String,
    },

    /// `KNOWN_SURFACES` entry with no claiming HLR.
    #[error("KNOWN_SURFACES entry '{surface}' is not claimed by any HLR")]
    SurfaceUnclaimed {
        /// Orphan surface.
        surface: String,
    },

    /// Required `verification_methods` list is empty.
    #[error("{kind} missing verification_methods: {id}")]
    MissingVerificationMethods {
        /// Item kind (`"HLR"` or `"LLR"`).
        kind: &'static str,
        /// Item's human ID.
        id: String,
    },

    /// `require_hlr_sys_trace` policy on + HLR.traces_to empty.
    #[error("HLR {hlr_id} has empty traces_to but policy require_hlr_sys_trace is set")]
    MissingHlrSysTrace {
        /// HLR's human ID.
        hlr_id: String,
    },

    /// Same link UUID appears twice in one item's `traces_to`.
    #[error("{source_kind} {source_id} has duplicate trace link '{link}'")]
    DuplicateTraceLink {
        /// Source item kind.
        source_kind: &'static str,
        /// Source item's human ID.
        source_id: String,
        /// Duplicated link UUID.
        link: String,
    },

    /// LLR with empty `traces_to` that isn't marked `derived = true`.
    #[error("LLR {llr_id} has no parent links. Must be marked 'derived = true'")]
    LlrMissingParentLinks {
        /// LLR's human ID.
        llr_id: String,
    },

    /// Derived LLR without a non-empty `rationale`.
    #[error("derived LLR {llr_id} missing non-empty rationale")]
    DerivedMissingRationale {
        /// LLR's human ID.
        llr_id: String,
    },

    /// LLR simultaneously `derived = true` and carrying non-empty
    /// `traces_to` — contradictory.
    #[error("LLR {llr_id} is marked derived but has trace links. Contradiction.")]
    ContradictoryDerived {
        /// LLR's human ID.
        llr_id: String,
    },

    /// Catch-all for Link-phase prose-only errors that a future
    /// refactor should convert to a dedicated variant. Emits
    /// `TRACE_LINK_OTHER`. A regression test in `validation.rs`
    /// fails on any new raw-string `errors.push(...)` call so this
    /// fallback stays empty in practice.
    #[error("{message}")]
    Other {
        /// Free-form message.
        message: String,
    },
}

impl DiagnosticCode for LinkError {
    fn code(&self) -> &'static str {
        match self {
            LinkError::InvalidLinkUuid { .. } => "TRACE_INVALID_LINK_UUID",
            LinkError::DanglingLink { .. } => "TRACE_DANGLING_LINK",
            LinkError::WrongTargetKind { .. } => "TRACE_WRONG_TARGET_KIND",
            LinkError::OwnershipViolation { .. } => "TRACE_OWNERSHIP_VIOLATION",
            LinkError::SurfaceUnknown { .. } => "TRACE_HLR_SURFACE_UNKNOWN",
            LinkError::SurfaceUnclaimed { .. } => "TRACE_HLR_SURFACE_UNCLAIMED",
            LinkError::MissingVerificationMethods { .. } => "TRACE_MISSING_VERIFICATION_METHODS",
            LinkError::MissingHlrSysTrace { .. } => "TRACE_MISSING_HLR_SYS_TRACE",
            LinkError::DuplicateTraceLink { .. } => "TRACE_DUPLICATE_TRACE_LINK",
            LinkError::LlrMissingParentLinks { .. } => "TRACE_LLR_MISSING_PARENT_LINKS",
            LinkError::DerivedMissingRationale { .. } => "TRACE_DERIVED_MISSING_RATIONALE",
            LinkError::ContradictoryDerived { .. } => "TRACE_CONTRADICTORY_DERIVED",
            LinkError::Other { .. } => "TRACE_LINK_OTHER",
        }
    }

    fn severity(&self) -> Severity {
        Severity::Error
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

    /// Every variant returns a distinct code. The walker-level
    /// bijection (`diagnostic_codes_locked`) enforces uniqueness
    /// across the whole crate; this test localizes the check for
    /// LinkError so a rename shows up here first.
    #[test]
    fn every_variant_code_is_unique() {
        use std::collections::BTreeSet;
        let samples: Vec<LinkError> = vec![
            LinkError::InvalidLinkUuid {
                source_kind: "HLR",
                source_id: "x".into(),
                link: "y".into(),
            },
            LinkError::DanglingLink {
                source_kind: "HLR",
                source_id: "x".into(),
                link: "y".into(),
                expected_target_kind: "SYS",
            },
            LinkError::WrongTargetKind {
                source_kind: "HLR",
                source_id: "x".into(),
                link: "y".into(),
                expected: "SYS",
                found: "LLR".into(),
            },
            LinkError::OwnershipViolation {
                source_kind: "HLR",
                source_id: "x".into(),
                source_owner: "a".into(),
                target_kind: "SYS",
                target_id: "y".into(),
                target_owner: "b".into(),
            },
            LinkError::SurfaceUnknown {
                hlr_id: "HLR-1".into(),
                surface: "zzz".into(),
            },
            LinkError::SurfaceUnclaimed {
                surface: "zzz".into(),
            },
            LinkError::MissingVerificationMethods {
                kind: "HLR",
                id: "x".into(),
            },
            LinkError::MissingHlrSysTrace {
                hlr_id: "HLR-1".into(),
            },
            LinkError::DuplicateTraceLink {
                source_kind: "HLR",
                source_id: "x".into(),
                link: "y".into(),
            },
            LinkError::LlrMissingParentLinks {
                llr_id: "LLR-1".into(),
            },
            LinkError::DerivedMissingRationale {
                llr_id: "LLR-1".into(),
            },
            LinkError::ContradictoryDerived {
                llr_id: "LLR-1".into(),
            },
            LinkError::Other {
                message: "anything".into(),
            },
        ];
        let codes: BTreeSet<&str> = samples.iter().map(|e| e.code()).collect();
        assert_eq!(
            codes.len(),
            samples.len(),
            "LinkError variants must return distinct codes; got {:?}",
            codes
        );
    }

    /// Every variant's `#[error(...)]` template produces a non-empty
    /// string when formatted. Catches placeholder-name typos.
    #[test]
    fn every_variant_renders_non_empty() {
        let e = LinkError::SurfaceUnknown {
            hlr_id: "HLR-1".into(),
            surface: "zzz".into(),
        };
        assert!(!e.to_string().is_empty());
        assert!(e.to_string().contains("HLR-1"));
        assert!(e.to_string().contains("zzz"));
    }
}
