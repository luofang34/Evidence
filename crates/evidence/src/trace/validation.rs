//! Cross-tier trace link validation (HLR → LLR → Test + Derived).
//!
//! The single shared pass enforces: uniqueness of every UID and every
//! `(kind, owner, id)` triple, UUID syntax, link-target existence,
//! link-target kind, ownership rules (LLR→HLR same-owner or
//! owner={soi,project}; TEST→LLR strictly same-owner), and a battery
//! of policy-gated checks (required uids / owners / rationale /
//! verification methods). Derived LLRs take the alternate branch where
//! `traces_to` is empty; the check flags the contradiction if both
//! `derived = true` and a non-empty `traces_to` are present.
//!
//! Orphan tests (empty `traces_to`) are a *warning* here. They are
//! listed in the traceability matrix's gap section instead of hard-
//! failing validation — a program with in-progress test work can
//! still ship a bundle, but the gap is visible to reviewers.

use anyhow::{Result, bail};
use std::collections::{BTreeMap, BTreeSet};

use super::entries::{DerivedEntry, HlrEntry, LlrEntry, TestEntry};
use crate::policy::TracePolicy;

/// Validate trace links between HLRs, LLRs, Tests, and optionally Derived requirements.
///
/// Convenience wrapper around [`validate_trace_links_with_policy`] that uses
/// the default `TracePolicy` and no derived requirements.
pub fn validate_trace_links(
    hlrs: &[HlrEntry],
    llrs: &[LlrEntry],
    tests: &[TestEntry],
) -> Result<()> {
    validate_trace_links_with_policy(hlrs, llrs, tests, &[], &TracePolicy::default())
}

/// Validate trace links with explicit policy control and derived requirements.
///
/// Policy fields are read, not written. Every error is accumulated
/// and logged before the final `bail!` so a single run surfaces all
/// issues rather than the first one. Two stages: `register` pass
/// (uniqueness + format) bails before link checks if it finds
/// anything, because downstream link checks assume the uid index is
/// consistent.
pub fn validate_trace_links_with_policy(
    hlrs: &[HlrEntry],
    llrs: &[LlrEntry],
    tests: &[TestEntry],
    derived: &[DerivedEntry],
    policy: &TracePolicy,
) -> Result<()> {
    let mut errors: Vec<String> = Vec::new();

    // Index: uid -> (kind, owner, id)
    let mut uid_index: BTreeMap<String, (String, String, String)> = BTreeMap::new();
    // Index: (kind, owner, id) -> uid (to check item uniqueness)
    let mut item_index: BTreeMap<(String, String, String), String> = BTreeMap::new();

    let mut register = |uid: &Option<String>, owner: &Option<String>, id: &String, kind: &str| {
        let o = if let Some(ow) = owner {
            ow.clone()
        } else {
            if policy.require_owners {
                errors.push(format!("[{}:{}] missing 'owner'", kind, id));
            }
            return;
        };

        let u = match uid {
            Some(u) => {
                if policy.require_uids && uuid::Uuid::parse_str(u).is_err() {
                    errors.push(format!("[{}:{}] invalid UID format '{}'", kind, id, u));
                    return;
                }
                u.clone()
            }
            None => {
                if policy.require_uids {
                    errors.push(format!("[{}:{}] missing UID", kind, id));
                }
                return;
            }
        };

        if let Some((prev_kind, prev_owner, prev_id)) = uid_index.get(&u) {
            errors.push(format!(
                "Duplicate UID {}: used by [{}({}):{}] and [{}({}):{}]",
                u, prev_kind, prev_owner, prev_id, kind, o, id
            ));
        } else {
            uid_index.insert(u.clone(), (kind.to_string(), o.clone(), id.clone()));
        }

        let key = (kind.to_string(), o.clone(), id.clone());
        if let Some(prev_uid) = item_index.get(&key) {
            errors.push(format!(
                "Duplicate Item '{}({}):{}': used by {} and {}",
                kind, o, id, prev_uid, u
            ));
        } else {
            item_index.insert(key, u);
        }
    };

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

    if !errors.is_empty() {
        for e in &errors {
            log::error!("  VALIDATION ERROR: {}", e);
        }
        bail!(
            "Validation failed with {} errors (fix before linking check)",
            errors.len()
        );
    }

    // Link Validation
    let check_link = |source_kind: &str,
                      source_id: &str,
                      source_owner: &Option<String>,
                      link: &str,
                      expected_target_kind: &str|
     -> Option<String> {
        // 1. Must be UUID.
        if uuid::Uuid::parse_str(link).is_err() {
            return Some(format!("Link '{}' in {} is not a UUID", link, source_id));
        }

        // 2. Must exist.
        let (target_kind, target_owner, target_id) = match uid_index.get(link) {
            Some(t) => t,
            None => {
                return Some(format!(
                    "Link '{}' in {} not found (dangling ref)",
                    link, source_id
                ));
            }
        };

        // 3. Kind check.
        if target_kind != expected_target_kind {
            return Some(format!(
                "Link '{}' in {} points to {} but expected {}",
                link, source_id, target_kind, expected_target_kind
            ));
        }

        // 4. Ownership logic.
        let s_owner = source_owner
            .as_ref()
            .map(|s| s.as_str())
            .unwrap_or("UNKNOWN");
        let t_owner = target_owner.as_str();

        match (source_kind, expected_target_kind) {
            ("LLR", "HLR") => {
                // Allowed: same owner OR target is "soi"/"project".
                if s_owner == t_owner || t_owner == "soi" || t_owner == "project" {
                    // OK
                } else {
                    return Some(format!(
                        "Ownership violation: LLR({}:{}) -> HLR({}:{}). Cross-crate link forbidden.",
                        s_owner, source_id, t_owner, target_id
                    ));
                }
            }
            ("TEST", "LLR") => {
                // Allowed: strictly same owner.
                if s_owner != t_owner {
                    return Some(format!(
                        "Ownership violation: TEST({}:{}) -> LLR({}:{}). Must be same crate.",
                        s_owner, source_id, t_owner, target_id
                    ));
                }
            }
            _ => { /* Checks not implemented for other pairings */ }
        }

        None
    };

    // Policy-gated checks

    // HLR policy.
    for r in hlrs {
        if policy.require_hlr_verification_methods && r.verification_methods.is_empty() {
            errors.push(format!("HLR missing verification_methods: {}", r.id));
        }
    }

    for r in llrs {
        // LLR policy: derived vs traced.
        if r.traces_to.is_empty() {
            if !r.derived {
                errors.push(format!(
                    "LLR {} has no parent links. Must be marked 'derived = true'",
                    r.id
                ));
            } else if policy.require_derived_rationale
                && r.rationale.as_ref().map(|s| s.is_empty()).unwrap_or(true)
            {
                errors.push(format!("Derived LLR {} missing 'rationale'", r.id));
            }
        } else if r.derived {
            errors.push(format!(
                "LLR {} is marked derived but has trace links. Contradiction.",
                r.id
            ));
        }

        if policy.require_llr_verification_methods && r.verification_methods.is_empty() {
            errors.push(format!("LLR missing verification_methods: {}", r.id));
        }

        let mut seen_links = BTreeSet::new();
        for link in &r.traces_to {
            if !seen_links.insert(link) {
                errors.push(format!("LLR {} has duplicate trace link '{}'", r.id, link));
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
                errors.push(format!("TEST {} has duplicate trace link '{}'", t.id, link));
            }
            if let Some(e) = check_link("TEST", &t.id, &t.owner, link, "LLR") {
                errors.push(e);
            }
        }
    }

    // Derived requirements validation.
    for d in derived {
        if policy.require_derived_rationale
            && d.rationale.as_ref().map(|s| s.is_empty()).unwrap_or(true)
        {
            errors.push(format!("Derived requirement {} missing 'rationale'", d.id));
        }
    }

    // Orphan test detection: tests with empty traces_to list.
    let orphan_tests: Vec<&TestEntry> = tests.iter().filter(|t| t.traces_to.is_empty()).collect();
    if !orphan_tests.is_empty() {
        for t in &orphan_tests {
            log::warn!("  WARNING: Orphan test '{}' is not linked to any LLR", t.id);
        }
        log::warn!(
            "  WARNING: {} orphan test(s) found (tests with no LLR link)",
            orphan_tests.len()
        );
    }

    if !errors.is_empty() {
        for e in &errors {
            log::error!("  LINK ERROR: {}", e);
        }
        bail!("Trace link validation failed with {} errors", errors.len());
    }

    Ok(())
}
