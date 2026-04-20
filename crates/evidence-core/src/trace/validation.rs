//! Cross-tier trace link validation (SYS → HLR → LLR → Test + Derived).
//!
//! The single shared pass enforces: uniqueness of every UID and every
//! `(kind, owner, id)` triple, UUID syntax, link-target existence,
//! link-target kind, ownership rules (HLR→SYS same-owner or
//! owner={soi,project}; LLR→HLR same-owner or owner={soi,project};
//! TEST→LLR strictly same-owner), and a battery of policy-gated
//! checks (required uids / owners / rationale / verification methods).
//! Derived LLRs take the alternate branch where `traces_to` is empty;
//! the check flags the contradiction if both `derived = true` and a
//! non-empty `traces_to` are present. HLR `traces_to` is optional —
//! an HLR with empty `traces_to` is tool-internal and legal (it does
//! not claim a System-Requirement parent). SYS entries reuse the HLR
//! struct shape; their `traces_to` is always empty because SYS is the
//! top of the chain.
//!
//! Orphan tests (empty `traces_to`) are a *warning* here. They are
//! listed in the traceability matrix's gap section instead of hard-
//! failing validation — a program with in-progress test work can
//! still ship a bundle, but the gap is visible to reviewers.

use std::collections::{BTreeMap, BTreeSet};

use thiserror::Error;

use super::entries::{DerivedEntry, HlrEntry, LlrEntry, TestEntry};
use crate::diagnostic::{DiagnosticCode, Severity};
use crate::policy::TracePolicy;

mod link_errors;
pub use link_errors::LinkError;

/// Errors returned by [`validate_trace_links`] / [`validate_trace_links_with_policy`].
///
/// Validation collects every problem in a single pass and returns a
/// single error with a list of issues. Two distinct phases, because
/// the link-validation phase assumes the UID index built by the
/// register phase is consistent — if any register-phase errors fired,
/// link checks would produce noise.
#[derive(Debug, Error)]
pub enum TraceValidationError {
    /// Register-phase failures (missing UIDs/owners, duplicate UIDs
    /// or `(kind, owner, id)` triples, malformed UUID strings). One
    /// string per violation in the `errors` vector. **Not promoted to
    /// typed variants** — scope was Link-phase only; the
    /// Register phase has similar opaque-prose issues but defers
    /// until MCP surfaces a concrete need.
    #[error("Validation failed with {} errors (fix before linking check)", errors.len())]
    Register {
        /// One free-form message per register-phase violation.
        errors: Vec<String>,
    },
    /// Link-phase failures: dangling trace refs, cross-owner links
    /// that violate ownership rules, missing verification methods,
    /// missing rationales on derived LLRs, etc. One
    /// [`LinkError`] per violation; iterate the vector to emit per-
    /// variant diagnostics with each variant's stable code.
    #[error("Trace link validation failed with {} errors", errors.len())]
    Link {
        /// Typed sub-errors, one per violation. Use `.code()` on
        /// each to get the per-variant `TRACE_*` code. Ordering
        /// matches the pass-through order of the validator; agents
        /// that want deterministic output sort client-side by code.
        errors: Vec<LinkError>,
    },
}

impl DiagnosticCode for TraceValidationError {
    fn code(&self) -> &'static str {
        match self {
            TraceValidationError::Register { .. } => "TRACE_REGISTER_FAILED",
            TraceValidationError::Link { .. } => "TRACE_LINK_FAILED",
        }
    }

    fn severity(&self) -> Severity {
        Severity::Error
    }
}

/// Validate trace links between HLRs, LLRs, Tests, and optionally Derived requirements.
///
/// Convenience wrapper around [`validate_trace_links_with_policy`] that uses
/// the default `TracePolicy`, no System Requirements, and no derived
/// requirements. Kept as a three-argument entry point for the common
/// case where callers don't have a SYS layer yet.
pub fn validate_trace_links(
    hlrs: &[HlrEntry],
    llrs: &[LlrEntry],
    tests: &[TestEntry],
) -> Result<(), TraceValidationError> {
    validate_trace_links_with_policy(&[], hlrs, llrs, tests, &[], &TracePolicy::default())
}

/// Validate trace links with explicit policy control, System
/// Requirements, and Derived Requirements.
///
/// `sys` carries System-Requirement entries loaded from `sys.toml`.
/// They use the same [`HlrEntry`] struct because SYS and HLR share
/// every field (the layer is signaled by the source filename).
/// HLRs may trace up to SYS UIDs via [`HlrEntry::traces_to`]; the
/// link check allows empty `traces_to` for tool-internal HLRs with
/// no System parent.
///
/// Policy fields are read, not written. Every error is accumulated
/// and logged before the final `bail!` so a single run surfaces all
/// issues rather than the first one. Two stages: `register` pass
/// (uniqueness + format) bails before link checks if it finds
/// anything, because downstream link checks assume the uid index is
/// consistent.
pub fn validate_trace_links_with_policy(
    sys: &[HlrEntry],
    hlrs: &[HlrEntry],
    llrs: &[LlrEntry],
    tests: &[TestEntry],
    derived: &[DerivedEntry],
    policy: &TracePolicy,
) -> Result<(), TraceValidationError> {
    let mut register_errors: Vec<String> = Vec::new();

    // Index: uid -> (kind, owner, id)
    let mut uid_index: BTreeMap<String, (String, String, String)> = BTreeMap::new();
    // Index: (kind, owner, id) -> uid (to check item uniqueness)
    let mut item_index: BTreeMap<(String, String, String), String> = BTreeMap::new();

    let mut register = |uid: &Option<String>, owner: &Option<String>, id: &String, kind: &str| {
        let o = if let Some(ow) = owner {
            ow.clone()
        } else {
            if policy.require_owners {
                register_errors.push(format!("[{}:{}] missing 'owner'", kind, id));
            }
            return;
        };

        let u = match uid {
            Some(u) => {
                if policy.require_uids && uuid::Uuid::parse_str(u).is_err() {
                    register_errors.push(format!("[{}:{}] invalid UID format '{}'", kind, id, u));
                    return;
                }
                u.clone()
            }
            None => {
                if policy.require_uids {
                    register_errors.push(format!("[{}:{}] missing UID", kind, id));
                }
                return;
            }
        };

        if let Some((prev_kind, prev_owner, prev_id)) = uid_index.get(&u) {
            register_errors.push(format!(
                "Duplicate UID {}: used by [{}({}):{}] and [{}({}):{}]",
                u, prev_kind, prev_owner, prev_id, kind, o, id
            ));
        } else {
            uid_index.insert(u.clone(), (kind.to_string(), o.clone(), id.clone()));
        }

        let key = (kind.to_string(), o.clone(), id.clone());
        if let Some(prev_uid) = item_index.get(&key) {
            register_errors.push(format!(
                "Duplicate Item '{}({}):{}': used by {} and {}",
                kind, o, id, prev_uid, u
            ));
        } else {
            item_index.insert(key, u);
        }
    };

    for r in sys {
        register(&r.uid, &r.owner, &r.id, "SYS");
    }
    for r in hlrs {
        register(&r.uid, &r.owner, &r.id, "HLR");
    }
    for r in llrs {
        register(&r.uid, &r.owner, &r.id, "LLR");
    }
    for t in tests {
        register(&t.uid, &t.owner, &t.id, "TEST");
    }
    for d in derived {
        register(&d.uid, &d.owner, &d.id, "DERIVED");
    }

    if !register_errors.is_empty() {
        for e in &register_errors {
            tracing::error!("  VALIDATION ERROR: {}", e);
        }
        return Err(TraceValidationError::Register {
            errors: register_errors,
        });
    }

    // Link Validation — each check returns a typed `LinkError`
    // variant; the outer loop iterates links and accumulates.
    let mut errors: Vec<LinkError> = Vec::new();

    let check_link = |source_kind: &'static str,
                      source_id: &str,
                      source_owner: &Option<String>,
                      link: &str,
                      expected_target_kind: &'static str|
     -> Option<LinkError> {
        // 1. Must be UUID.
        if uuid::Uuid::parse_str(link).is_err() {
            return Some(LinkError::InvalidLinkUuid {
                source_kind,
                source_id: source_id.to_string(),
                link: link.to_string(),
            });
        }

        // 2. Must exist.
        let (target_kind, target_owner, target_id) = match uid_index.get(link) {
            Some(t) => t,
            None => {
                return Some(LinkError::DanglingLink {
                    source_kind,
                    source_id: source_id.to_string(),
                    link: link.to_string(),
                    expected_target_kind,
                });
            }
        };

        // 3. Kind check.
        if target_kind != expected_target_kind {
            return Some(LinkError::WrongTargetKind {
                source_kind,
                source_id: source_id.to_string(),
                link: link.to_string(),
                expected: expected_target_kind,
                found: target_kind.clone(),
            });
        }

        // 4. Ownership logic.
        let s_owner = source_owner
            .as_ref()
            .map(|s| s.as_str())
            .unwrap_or("UNKNOWN");
        let t_owner = target_owner.as_str();

        let mk_ownership = || LinkError::OwnershipViolation {
            source_kind,
            source_id: source_id.to_string(),
            source_owner: s_owner.to_string(),
            target_kind: expected_target_kind,
            target_id: target_id.clone(),
            target_owner: t_owner.to_string(),
        };

        match (source_kind, expected_target_kind) {
            ("HLR", "SYS") | ("LLR", "HLR") => {
                // Allowed: same owner OR target is "soi"/"project".
                // System requirements commonly have owner `soi` (system-
                // of-interest) since they sit above component boundaries.
                if !(s_owner == t_owner || t_owner == "soi" || t_owner == "project") {
                    return Some(mk_ownership());
                }
            }
            ("TEST", "LLR") => {
                // Allowed: strictly same owner.
                if s_owner != t_owner {
                    return Some(mk_ownership());
                }
            }
            _ => { /* Checks not implemented for other pairings */ }
        }

        None
    };

    // Policy-gated checks

    // HLR policy + HLR→SYS link validation.
    // HLR-038 / LLR-038: HLR.surfaces ⇔ KNOWN_SURFACES
    // bijection. Every HLR.surfaces string must be in KNOWN_SURFACES;
    // every KNOWN_SURFACES entry must be claimed by ≥1 HLR. Policy-
    // gated so external traces (e.g. the test-harness tempdirs and
    // any downstream project that hasn't authored surfaces) stay
    // validating.
    if policy.require_hlr_surface_bijection {
        let known: BTreeSet<&str> = super::surfaces::KNOWN_SURFACES.iter().copied().collect();
        let mut claimed: BTreeSet<String> = BTreeSet::new();
        for r in hlrs {
            for s in &r.surfaces {
                if !known.contains(s.as_str()) {
                    errors.push(LinkError::SurfaceUnknown {
                        hlr_id: r.id.clone(),
                        surface: s.clone(),
                    });
                }
                claimed.insert(s.clone());
            }
        }
        for k in super::surfaces::KNOWN_SURFACES {
            if !claimed.contains(*k) {
                errors.push(LinkError::SurfaceUnclaimed {
                    surface: (*k).to_string(),
                });
            }
        }
    }

    for r in hlrs {
        if policy.require_hlr_verification_methods && r.verification_methods.is_empty() {
            errors.push(LinkError::MissingVerificationMethods {
                kind: "HLR",
                id: r.id.clone(),
            });
        }

        // HLR.traces_to is optional by default: empty = tool-internal
        // HLR with no System-Requirement parent (legal). When
        // require_hlr_sys_trace is set, empty becomes a Link-phase
        // error — the gate that turns the SYS layer from advisory
        // into load-bearing for projects that opt in.
        if policy.require_hlr_sys_trace && r.traces_to.is_empty() {
            errors.push(LinkError::MissingHlrSysTrace {
                hlr_id: r.id.clone(),
            });
        }

        // Non-empty traces_to must resolve to SYS UIDs and obey
        // ownership rules.
        let mut seen_links = BTreeSet::new();
        for link in &r.traces_to {
            if !seen_links.insert(link) {
                errors.push(LinkError::DuplicateTraceLink {
                    source_kind: "HLR",
                    source_id: r.id.clone(),
                    link: link.clone(),
                });
            }
            if let Some(e) = check_link("HLR", &r.id, &r.owner, link, "SYS") {
                errors.push(e);
            }
        }
    }

    for r in llrs {
        // LLR policy: derived vs traced.
        if r.traces_to.is_empty() {
            if !r.derived {
                errors.push(LinkError::LlrMissingParentLinks {
                    llr_id: r.id.clone(),
                });
            } else if r.rationale.as_ref().map(|s| s.is_empty()).unwrap_or(true) {
                // DO-178C §5.2.2 — derived requirements must carry a
                // non-empty rationale. Unconditional rule (no policy
                // gate); HLR-040.
                errors.push(LinkError::DerivedMissingRationale {
                    llr_id: r.id.clone(),
                });
            }
        } else if r.derived {
            errors.push(LinkError::ContradictoryDerived {
                llr_id: r.id.clone(),
            });
        }

        if policy.require_llr_verification_methods && r.verification_methods.is_empty() {
            errors.push(LinkError::MissingVerificationMethods {
                kind: "LLR",
                id: r.id.clone(),
            });
        }

        let mut seen_links = BTreeSet::new();
        for link in &r.traces_to {
            if !seen_links.insert(link) {
                errors.push(LinkError::DuplicateTraceLink {
                    source_kind: "LLR",
                    source_id: r.id.clone(),
                    link: link.clone(),
                });
            }
            if let Some(e) = check_link("LLR", &r.id, &r.owner, link, "HLR") {
                errors.push(e);
            }
        }
    }
    for t in tests {
        let mut seen_links = BTreeSet::new();
        for link in &t.traces_to {
            if !seen_links.insert(link) {
                errors.push(LinkError::DuplicateTraceLink {
                    source_kind: "TEST",
                    source_id: t.id.clone(),
                    link: link.clone(),
                });
            }
            if let Some(e) = check_link("TEST", &t.id, &t.owner, link, "LLR") {
                errors.push(e);
            }
        }
    }

    // Derived top-level entries. Shares the `DerivedMissingRationale`
    // code with LLR-derived entries — same semantic, different source
    // stream; agents keyed on `code` see both.
    for d in derived {
        if policy.require_derived_rationale
            && d.rationale.as_ref().map(|s| s.is_empty()).unwrap_or(true)
        {
            errors.push(LinkError::DerivedMissingRationale {
                llr_id: d.id.clone(),
            });
        }
    }

    // Orphan test detection: tests with empty traces_to list.
    let orphan_tests: Vec<&TestEntry> = tests.iter().filter(|t| t.traces_to.is_empty()).collect();
    if !orphan_tests.is_empty() {
        for t in &orphan_tests {
            tracing::warn!("  WARNING: Orphan test '{}' is not linked to any LLR", t.id);
        }
        tracing::warn!(
            "  WARNING: {} orphan test(s) found (tests with no LLR link)",
            orphan_tests.len()
        );
    }

    if !errors.is_empty() {
        for e in &errors {
            tracing::error!("  LINK ERROR: {}", e);
        }
        return Err(TraceValidationError::Link { errors });
    }

    Ok(())
}
