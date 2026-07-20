//! Integration tests for the Link-phase decomposition rules:
//!
//! - `HlrEntry.surfaces` ⇔ `KNOWN_SURFACES` bijection
//!   → `LinkError::SurfaceUnknown` (`TRACE_HLR_SURFACE_UNKNOWN`)
//!   + `LinkError::SurfaceUnclaimed` (`TRACE_HLR_SURFACE_UNCLAIMED`).
//! - `TestEntry.test_selectors: Vec<String>` with `StringOrVec`
//!   deserializer (single-string shorthand round-trips to
//!   multi-element array semantics).
//! - Derived LLR without rationale
//!   → `LinkError::DerivedMissingRationale`
//!   (`TRACE_DERIVED_MISSING_RATIONALE`).
//! - Derived completeness gates (DAL-C+, LLR-106)
//!   → `LinkError::DerivedMissingSafetyImpact`
//!   (`TRACE_DERIVED_MISSING_SAFETY_IMPACT`),
//!   `LinkError::DerivedMissingDisposition`
//!   (`TRACE_DERIVED_MISSING_DISPOSITION`),
//!   `LinkError::DerivedUnreviewed` (`TRACE_DERIVED_UNREVIEWED`).
//!
//! Assertions match on `err.code()` returns.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use evidence_core::TracePolicy;
use evidence_core::diagnostic::DiagnosticCode;
use evidence_core::trace::{
    DerivedEntry, HlrEntry, LinkError, LlrEntry, TestEntry, TraceValidationError,
    validate_trace_links_with_policy,
};

fn hlr(id: &str, uid: &str, traces_to: Vec<String>, surfaces: Vec<String>) -> HlrEntry {
    HlrEntry {
        uid: Some(uid.into()),
        ns: None,
        id: id.into(),
        title: format!("title for {}", id),
        owner: Some("tool".into()),
        scope: None,
        sort_key: None,
        category: None,
        source: None,
        description: None,
        rationale: None,
        verification_methods: vec![],
        traces_to,
        surfaces,
    }
}

fn llr(id: &str, uid: &str, traces_to: Vec<String>) -> LlrEntry {
    LlrEntry {
        uid: Some(uid.into()),
        ns: None,
        id: id.into(),
        title: format!("title for {}", id),
        owner: Some("tool".into()),
        sort_key: None,
        traces_to,
        source: None,
        modules: vec![],
        description: None,
        verification_methods: vec!["test".into()],
        emits: vec![],
    }
}

/// `surface_unknown_fires_with_typed_code`: an HLR claiming a
/// surface not in KNOWN_SURFACES must fire
/// `TRACE_HLR_SURFACE_UNKNOWN` as a typed `LinkError`, with the
/// offending `(hlr_id, surface)` reachable via the variant payload.
///
/// Covered-by-design: `surface_unclaimed_fires_with_typed_code`
/// fires in the same `validate_trace_links_with_policy` run
/// because the fixture's HLR only claims ONE known surface
/// (`check`), leaving every other `KNOWN_SURFACES` entry
/// orphaned. Both variants surface in the errors vec; this test
/// asserts the unknown arm, the companion test below asserts the
/// unclaimed arm. Separating them lets the failure mode read
/// "one specific arm broke," not "something in the bijection did."
#[test]
fn surface_unknown_fires_with_typed_code() {
    // Single HLR claiming (a) a surface that IS in KNOWN_SURFACES
    // (covers one of them but leaves others unclaimed), and (b) a
    // surface that is NOT in KNOWN_SURFACES (unknown).
    let h = hlr(
        "HLR-1",
        "aaaaaaaa-0000-4000-8000-000000000001",
        vec![],
        vec!["check".into(), "NOT_A_REAL_SURFACE".into()],
    );
    let l = llr(
        "LLR-1",
        "aaaaaaaa-0000-4000-8000-000000000002",
        vec!["aaaaaaaa-0000-4000-8000-000000000001".into()],
    );
    let t = TestEntry {
        uid: Some("aaaaaaaa-0000-4000-8000-000000000003".into()),
        ns: None,
        id: "TEST-1".into(),
        title: "t".into(),
        owner: Some("tool".into()),
        sort_key: None,
        traces_to: vec!["aaaaaaaa-0000-4000-8000-000000000002".into()],
        description: None,
        category: None,
        test_selector: Some("t".into()),
        test_selectors: vec![],
        source: None,
    };

    let policy = TracePolicy {
        require_hlr_surface_bijection: true,
        ..TracePolicy::default()
    };
    let err = validate_trace_links_with_policy(&[], &[h], &[l], &[t], &[], &policy)
        .expect_err("expected bijection failure");

    let TraceValidationError::Link { errors } = err else {
        panic!("expected Link variant, got {:?}", err);
    };
    let codes: Vec<&str> = errors.iter().map(|e| e.code()).collect();
    assert!(
        codes.contains(&"TRACE_HLR_SURFACE_UNKNOWN"),
        "expected TRACE_HLR_SURFACE_UNKNOWN for 'NOT_A_REAL_SURFACE'; got codes:\n{:?}",
        codes
    );
    let unknown_payload = errors.iter().find_map(|e| match e {
        LinkError::SurfaceUnknown { hlr_id, surface } => Some((hlr_id.clone(), surface.clone())),
        _ => None,
    });
    assert_eq!(
        unknown_payload,
        Some(("HLR-1".into(), "NOT_A_REAL_SURFACE".into())),
        "SurfaceUnknown payload must carry the offending (hlr_id, surface)"
    );
}

/// `surface_unclaimed_fires_with_typed_code`: companion to the
/// unknown arm — asserts `TRACE_HLR_SURFACE_UNCLAIMED` fires for
/// every `KNOWN_SURFACES` entry not claimed by any HLR. Uses the
/// same minimal fixture: one HLR claims a single known surface,
/// leaving every other entry in `KNOWN_SURFACES` orphaned.
#[test]
fn surface_unclaimed_fires_with_typed_code() {
    let h = hlr(
        "HLR-1",
        "bbbbbbbb-0000-4000-8000-000000000001",
        vec![],
        vec!["check".into()],
    );
    let l = llr(
        "LLR-1",
        "bbbbbbbb-0000-4000-8000-000000000002",
        vec!["bbbbbbbb-0000-4000-8000-000000000001".into()],
    );
    let t = TestEntry {
        uid: Some("bbbbbbbb-0000-4000-8000-000000000003".into()),
        ns: None,
        id: "TEST-1".into(),
        title: "t".into(),
        owner: Some("tool".into()),
        sort_key: None,
        traces_to: vec!["bbbbbbbb-0000-4000-8000-000000000002".into()],
        description: None,
        category: None,
        test_selector: Some("t".into()),
        test_selectors: vec![],
        source: None,
    };

    let policy = TracePolicy {
        require_hlr_surface_bijection: true,
        ..TracePolicy::default()
    };
    let err = validate_trace_links_with_policy(&[], &[h], &[l], &[t], &[], &policy)
        .expect_err("expected bijection failure");

    let TraceValidationError::Link { errors } = err else {
        panic!("expected Link variant, got {:?}", err);
    };
    let codes: Vec<&str> = errors.iter().map(|e| e.code()).collect();
    assert!(
        codes.contains(&"TRACE_HLR_SURFACE_UNCLAIMED"),
        "expected TRACE_HLR_SURFACE_UNCLAIMED for orphan KNOWN_SURFACES entries; got codes:\n{:?}",
        codes
    );
    // Payload-preservation: at least one unclaimed-surface payload
    // names a real `KNOWN_SURFACES` entry.
    let unclaimed_surfaces: Vec<String> = errors
        .iter()
        .filter_map(|e| match e {
            LinkError::SurfaceUnclaimed { surface } => Some(surface.clone()),
            _ => None,
        })
        .collect();
    assert!(
        !unclaimed_surfaces.is_empty(),
        "expected at least one SurfaceUnclaimed variant; got errors:\n{:?}",
        errors
    );
    assert!(
        unclaimed_surfaces
            .iter()
            .any(|s| evidence_core::trace::KNOWN_SURFACES.contains(&s.as_str())),
        "unclaimed surfaces must match real KNOWN_SURFACES entries; got {:?}",
        unclaimed_surfaces
    );
}

/// `TestEntry` expresses N:M mapping via the `test_selectors` Vec
/// alongside the legacy `test_selector` field. A single TEST entry
/// can claim multiple selectors; `all_selectors()` deduplicates and
/// returns them sorted. Pins the additive-widening contract.
#[test]
fn test_selectors_deserializes_both_shapes() {
    // Legacy shape — only `test_selector`.
    let legacy_toml = r#"
id = "TEST-legacy"
title = "legacy"
traces_to = []
test_selector = "foo::bar"
"#;
    let legacy: TestEntry = toml::from_str(legacy_toml).expect("legacy parses");
    assert_eq!(legacy.all_selectors(), vec!["foo::bar".to_string()]);

    // New shape — only `test_selectors` (Vec).
    let vec_toml = r#"
id = "TEST-vec"
title = "vec"
traces_to = []
test_selectors = ["foo::fn_a", "foo::fn_b"]
"#;
    let v: TestEntry = toml::from_str(vec_toml).expect("vec parses");
    assert_eq!(
        v.all_selectors(),
        vec!["foo::fn_a".to_string(), "foo::fn_b".to_string()]
    );

    // Union — both fields set; duplicates dropped, sort deterministic.
    let union_toml = r#"
id = "TEST-union"
title = "union"
traces_to = []
test_selector = "foo::single"
test_selectors = ["foo::single", "foo::extra"]
"#;
    let u: TestEntry = toml::from_str(union_toml).expect("union parses");
    assert_eq!(
        u.all_selectors(),
        vec!["foo::extra".to_string(), "foo::single".to_string()]
    );
}

// The legacy `LlrEntry.derived = true` + missing-rationale pathway
// was retired; the DO-178C §5.2.4 derived-requirement carve-out now
// lives exclusively in `cert/trace/derived.toml` (`DerivedEntry`).
// Equivalent end-to-end coverage of the DerivedEntry pathway lives
// in `crates/cargo-evidence/tests/derived_trace_validation.rs`
// (TEST-055).

/// Build a `DerivedEntry` with every completeness field populated;
/// individual tests clear one field to fire its gate.
fn complete_derived() -> DerivedEntry {
    DerivedEntry {
        uid: Some("aaaaaaaa-0000-4000-8000-0000000000dd".into()),
        id: "DERIVED-1".into(),
        title: "derived requirement".into(),
        owner: Some("tool".into()),
        source: Some("design".into()),
        description: None,
        rationale: Some("no parent covers this choice".into()),
        safety_impact: Some("low".into()),
        disposition: Some("notified to systems process; recorded here".into()),
        reviewed: Some(true),
        sort_key: None,
    }
}

/// Run the derived-only validation under a policy with every
/// derived gate enabled (mirrors DAL-C+), returning the LinkError
/// vec. The fixture feeds no other entries so the only errors that
/// can fire are the derived gates.
fn validate_derived(entry: &DerivedEntry) -> Vec<LinkError> {
    let policy = TracePolicy {
        require_derived_rationale: true,
        require_derived_safety_impact: true,
        require_derived_disposition: true,
        require_derived_reviewed: true,
        ..TracePolicy::default()
    };
    let err =
        validate_trace_links_with_policy(&[], &[], &[], &[], std::slice::from_ref(entry), &policy)
            .expect_err("fixture must fail exactly one derived gate");
    match err {
        TraceValidationError::Link { errors } => errors,
        other => panic!("expected Link-phase errors, got {other:?}"),
    }
}

/// DAL-C+ policy + derived entry without `disposition` →
/// `TRACE_DERIVED_MISSING_DISPOSITION` (typed, payload carries the
/// derived id).
#[test]
fn derived_missing_disposition_fires_with_typed_code() {
    let mut entry = complete_derived();
    entry.disposition = None;
    let errors = validate_derived(&entry);
    let payload = errors.iter().find_map(|e| match e {
        LinkError::DerivedMissingDisposition { derived_id } => Some(derived_id.clone()),
        _ => None,
    });
    assert_eq!(
        payload,
        Some("DERIVED-1".to_string()),
        "expected DerivedMissingDisposition for DERIVED-1; got errors:\n{errors:?}"
    );
    let codes: Vec<&str> = errors.iter().map(|e| e.code()).collect();
    assert!(
        codes.contains(&"TRACE_DERIVED_MISSING_DISPOSITION"),
        "typed code must be emitted; got {codes:?}"
    );
}

/// DAL-C+ policy + derived entry with `reviewed = false` →
/// `TRACE_DERIVED_UNREVIEWED`. The gate fails closed on anything
/// but an explicit `reviewed = true`.
#[test]
fn derived_unreviewed_fires_with_typed_code() {
    let mut entry = complete_derived();
    entry.reviewed = Some(false);
    let errors = validate_derived(&entry);
    let payload = errors.iter().find_map(|e| match e {
        LinkError::DerivedUnreviewed { derived_id } => Some(derived_id.clone()),
        _ => None,
    });
    assert_eq!(
        payload,
        Some("DERIVED-1".to_string()),
        "expected DerivedUnreviewed for DERIVED-1; got errors:\n{errors:?}"
    );
}

/// DAL-C+ policy + derived entry without `safety_impact` →
/// `TRACE_DERIVED_MISSING_SAFETY_IMPACT`.
#[test]
fn derived_missing_safety_impact_fires_with_typed_code() {
    let mut entry = complete_derived();
    entry.safety_impact = None;
    let errors = validate_derived(&entry);
    let payload = errors.iter().find_map(|e| match e {
        LinkError::DerivedMissingSafetyImpact { derived_id } => Some(derived_id.clone()),
        _ => None,
    });
    assert_eq!(
        payload,
        Some("DERIVED-1".to_string()),
        "expected DerivedMissingSafetyImpact for DERIVED-1; got errors:\n{errors:?}"
    );
}

/// Control: the fully-populated derived entry passes every gate
/// under the same DAL-C+ policy — proves each gate keys on its own
/// field and the negative tests above aren't failing vacuously.
#[test]
fn derived_complete_entry_passes_all_gates() {
    let entry = complete_derived();
    let policy = TracePolicy {
        require_derived_rationale: true,
        require_derived_safety_impact: true,
        require_derived_disposition: true,
        require_derived_reviewed: true,
        ..TracePolicy::default()
    };
    assert!(
        validate_trace_links_with_policy(&[], &[], &[], &[], std::slice::from_ref(&entry), &policy)
            .is_ok(),
        "complete derived entry must pass all gates"
    );
}
