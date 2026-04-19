//! Integration tests for the PR #44 SYS layer addition.
//!
//! The tests live here (rather than as a `mod tests` inside
//! `validation.rs`) because `validation.rs` is already near the
//! 500-line file-size limit — see `tests/file_size_limit.rs`. The
//! tests exercise only the public API (`validate_trace_links_with_policy`)
//! so integration-test placement is a clean fit, not a workaround.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use evidence::TracePolicy;
use evidence::trace::{HlrEntry, LlrEntry, TestEntry, validate_trace_links_with_policy};

/// Minimal `HlrEntry` stub. Shared by SYS (same struct) and HLR.
fn stub_hlr(uid: &str, id: &str, owner: &str, traces_to: Vec<String>) -> HlrEntry {
    HlrEntry {
        uid: Some(uid.to_string()),
        ns: None,
        id: id.to_string(),
        title: format!("title for {}", id),
        owner: Some(owner.to_string()),
        scope: None,
        sort_key: None,
        category: None,
        source: None,
        description: None,
        rationale: None,
        verification_methods: vec![],
        traces_to,
    }
}

fn stub_llr(uid: &str, id: &str, owner: &str, traces_to: Vec<String>) -> LlrEntry {
    LlrEntry {
        uid: Some(uid.to_string()),
        ns: None,
        id: id.to_string(),
        title: format!("title for {}", id),
        owner: Some(owner.to_string()),
        sort_key: None,
        traces_to,
        source: None,
        modules: vec![],
        derived: false,
        description: None,
        rationale: None,
        verification_methods: vec!["test".to_string()],
    }
}

fn stub_test(uid: &str, id: &str, owner: &str, traces_to: Vec<String>) -> TestEntry {
    TestEntry {
        uid: Some(uid.to_string()),
        ns: None,
        id: id.to_string(),
        title: format!("title for {}", id),
        owner: Some(owner.to_string()),
        sort_key: None,
        traces_to,
        description: None,
        category: None,
        test_selector: None,
        source: None,
    }
}

/// A complete SYS → HLR → LLR → Test chain validates cleanly.
/// Guards the PR #44 format extension end-to-end.
#[test]
fn sys_hlr_llr_test_chain_validates() {
    let sys_uuid = uuid::Uuid::new_v4().to_string();
    let hlr_uuid = uuid::Uuid::new_v4().to_string();
    let llr_uuid = uuid::Uuid::new_v4().to_string();
    let test_uuid = uuid::Uuid::new_v4().to_string();

    let sys = vec![stub_hlr(&sys_uuid, "SYS-001", "soi", vec![])];
    let hlrs = vec![stub_hlr(
        &hlr_uuid,
        "HLR-001",
        "tool",
        vec![sys_uuid.clone()],
    )];
    let llrs = vec![stub_llr(
        &llr_uuid,
        "LLR-001",
        "tool",
        vec![hlr_uuid.clone()],
    )];
    let tests = vec![stub_test(
        &test_uuid,
        "TEST-001",
        "tool",
        vec![llr_uuid.clone()],
    )];

    let result =
        validate_trace_links_with_policy(&sys, &hlrs, &llrs, &tests, &[], &TracePolicy::default());
    assert!(
        result.is_ok(),
        "SYS→HLR→LLR→Test chain should validate: {:?}",
        result.err(),
    );
}

/// An HLR that traces_to a dangling (non-existent) SYS UID fails
/// validation with a clear link-phase error.
#[test]
fn hlr_traces_to_dangling_sys_uid_fails() {
    let hlr_uuid = uuid::Uuid::new_v4().to_string();
    let bogus_sys_uuid = uuid::Uuid::new_v4().to_string();

    let hlrs = vec![stub_hlr(&hlr_uuid, "HLR-001", "tool", vec![bogus_sys_uuid])];

    let result = validate_trace_links_with_policy(
        &[], // no SYS entries — the referenced UID doesn't exist
        &hlrs,
        &[],
        &[],
        &[],
        &TracePolicy::default(),
    );
    let err = result.expect_err("expected Link phase failure");
    // Match on the public error surface — Display includes the
    // phase label and count.
    let msg = err.to_string();
    assert!(
        msg.contains("Trace link validation failed"),
        "expected Link phase error, got: {}",
        msg
    );
}

/// An HLR with empty `traces_to` is legal by default (tool-internal
/// HLR with no System-Requirement parent). Must not produce any
/// link-phase errors on its own under the default policy.
#[test]
fn hlr_with_empty_traces_to_is_legal() {
    let hlr_uuid = uuid::Uuid::new_v4().to_string();
    let hlrs = vec![stub_hlr(&hlr_uuid, "HLR-001", "tool", vec![])];

    let result =
        validate_trace_links_with_policy(&[], &hlrs, &[], &[], &[], &TracePolicy::default());
    assert!(
        result.is_ok(),
        "HLR with empty traces_to should validate: {:?}",
        result.err(),
    );
}

/// TEST-021: When `require_hlr_sys_trace` is set, an HLR with empty
/// `traces_to` fails Link-phase validation. This is the gate that
/// turns the SYS layer from advisory into load-bearing.
#[test]
fn require_hlr_sys_trace_rejects_empty_traces_to() {
    let hlr_uuid = uuid::Uuid::new_v4().to_string();
    let hlrs = vec![stub_hlr(&hlr_uuid, "HLR-001", "tool", vec![])];

    let policy = TracePolicy {
        require_hlr_sys_trace: true,
        ..TracePolicy::default()
    };

    let result = validate_trace_links_with_policy(&[], &hlrs, &[], &[], &[], &policy);
    let err = result.expect_err("policy gate must reject empty traces_to");
    let msg = err.to_string();
    assert!(
        msg.contains("Trace link validation failed"),
        "expected Link-phase error, got: {}",
        msg,
    );
}

/// TEST-021 (pair): When `require_hlr_sys_trace` is set AND the HLR
/// traces up to a SYS UID, validation still passes. Guards against a
/// regression where the gate fires even on populated HLRs.
#[test]
fn require_hlr_sys_trace_allows_populated_hlr() {
    let sys_uuid = uuid::Uuid::new_v4().to_string();
    let hlr_uuid = uuid::Uuid::new_v4().to_string();

    let sys = vec![stub_hlr(&sys_uuid, "SYS-001", "soi", vec![])];
    let hlrs = vec![stub_hlr(
        &hlr_uuid,
        "HLR-001",
        "tool",
        vec![sys_uuid.clone()],
    )];

    let policy = TracePolicy {
        require_hlr_sys_trace: true,
        ..TracePolicy::default()
    };

    let result = validate_trace_links_with_policy(&sys, &hlrs, &[], &[], &[], &policy);
    assert!(
        result.is_ok(),
        "HLR with populated traces_to should validate even under \
         require_hlr_sys_trace: {:?}",
        result.err(),
    );
}
