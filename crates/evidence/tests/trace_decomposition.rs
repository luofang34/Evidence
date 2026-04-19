//! Integration tests for PR #49's trace-schema hardening.
//!
//! Covers the three new Link-phase rules:
//!
//! - `HlrEntry.surfaces` ⇔ `KNOWN_SURFACES` bijection
//!   (TRACE_HLR_SURFACE_UNCLAIMED + TRACE_HLR_SURFACE_UNKNOWN).
//! - `TestEntry.test_selectors: Vec<String>` with `StringOrVec`
//!   deserializer (single-string shorthand round-trips to
//!   multi-element array semantics).
//! - Derived LLR without rationale fires
//!   TRACE_DERIVED_MISSING_RATIONALE.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use evidence::TracePolicy;
use evidence::trace::{HlrEntry, LlrEntry, TestEntry, validate_trace_links_with_policy};

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
        derived: false,
        description: None,
        rationale: None,
        verification_methods: vec!["test".into()],
        emits: vec![],
    }
}

/// Surface bijection fires in both directions: (a) an HLR that claims
/// a surface not in KNOWN_SURFACES trips TRACE_HLR_SURFACE_UNKNOWN;
/// (b) a KNOWN_SURFACES entry with no claiming HLR trips
/// TRACE_HLR_SURFACE_UNCLAIMED. Synthesizes a minimal trace that
/// exercises both arms in one validator run.
#[test]
fn surfaces_bijection_fires_on_orphan_and_unknown() {
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

    let msg = format!("{:?}", err);
    assert!(
        msg.contains("TRACE_HLR_SURFACE_UNKNOWN"),
        "expected UNKNOWN for 'NOT_A_REAL_SURFACE'; got:\n{}",
        msg
    );
    assert!(
        msg.contains("TRACE_HLR_SURFACE_UNCLAIMED"),
        "expected UNCLAIMED for other KNOWN_SURFACES entries; got:\n{}",
        msg
    );
}

/// `TestEntry` expresses N:M mapping via the `test_selectors` Vec
/// alongside the legacy `test_selector` field. A single TEST entry
/// can claim multiple selectors; `all_selectors()` deduplicates and
/// returns them sorted. Pins the PR #49 additive-widening contract.
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

/// Derived LLR without rationale fires
/// TRACE_DERIVED_MISSING_RATIONALE. Unconditional rule — no
/// `policy.require_derived_rationale` flag needed.
#[test]
fn derived_without_rationale_fires() {
    // Build a tree where the derived LLR has no traces_to and no
    // rationale. Also need enough surrounding structure to trip the
    // surface bijection into silence — easiest: zero HLRs, zero
    // tests, one derived LLR.
    let l = LlrEntry {
        uid: Some("bbbbbbbb-0000-4000-8000-000000000001".into()),
        ns: None,
        id: "LLR-1".into(),
        title: "derived without rationale".into(),
        owner: Some("tool".into()),
        sort_key: None,
        traces_to: vec![],
        source: None,
        modules: vec![],
        derived: true,
        description: None,
        rationale: None,
        verification_methods: vec!["test".into()],
        emits: vec![],
    };

    let err = validate_trace_links_with_policy(&[], &[], &[l], &[], &[], &TracePolicy::default())
        .expect_err("expected derived-rationale failure");

    let msg = format!("{:?}", err);
    assert!(
        msg.contains("TRACE_DERIVED_MISSING_RATIONALE"),
        "expected TRACE_DERIVED_MISSING_RATIONALE; got:\n{}",
        msg
    );
}
