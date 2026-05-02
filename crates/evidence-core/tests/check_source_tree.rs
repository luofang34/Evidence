//! Library-level tests for `build_requirement_report` — the
//! source-mode core of `cargo evidence check` (LLR-026,
//! LLR-027, TEST-025..027).
//!
//! These tests live on the library side (not `crates/cargo-evidence/
//! tests/`) because the CLI-level source mode would spawn
//! `cargo test --workspace` and recurse inside the test binary. The
//! library function takes a synthetic outcome map directly so we can
//! pin per-requirement diagnostic shape without running cargo twice.
//!
//! What these tests pin:
//!
//! - TEST-025: a clean trace with every selector passing yields
//!   all-`REQ_PASS`.
//! - TEST-026: a mutated trace (empty HLR.traces_to) produces
//!   `REQ_GAP` carrying a `FixHint::AddTomlKey` that points at the
//!   traces_to field — the autofix contract MCP will depend
//!   on.
//! - TEST-027: a failing leaf TEST produces `REQ_GAP`s at upstream
//!   LLR/HLR/SYS entries, each carrying `root_cause_uid` pointing at
//!   the failing TEST's UID.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use std::collections::BTreeMap;
use std::path::Path;

use evidence_core::bundle::TestOutcome;
use evidence_core::diagnostic::FixHint;
use evidence_core::policy::TracePolicy;
use evidence_core::trace::{
    HlrEntry, HlrFile, LlrEntry, LlrFile, Schema, TestEntry, TestsFile, TraceMeta,
    build_requirement_report,
};

// The library doesn't expose TraceFiles' constructor directly (it's
// built by `read_all_trace_files`), so we synthesize one by reading
// a tiny on-disk fixture built per test.
fn synth_trace(
    sys: Vec<HlrEntry>,
    hlr: Vec<HlrEntry>,
    llr: Vec<LlrEntry>,
    tests: Vec<TestEntry>,
) -> evidence_core::trace::TraceFiles {
    // `TraceFiles::new` isn't public; write to a temp dir and read
    // back via the public loader. Keeps this test aligned with real
    // production parsing (catches a future change in the loader).
    use std::fs;
    use tempfile::TempDir;
    let td = TempDir::new().unwrap();
    let root = td.path();
    let schema = Schema {
        version: evidence_core::schema_versions::TRACE.to_string(),
    };
    let meta = TraceMeta {
        document_id: "test".into(),
        revision: "1.0".into(),
    };
    fs::write(
        root.join("sys.toml"),
        toml::to_string(&HlrFile {
            schema: schema.clone(),
            meta: meta.clone(),
            requirements: sys,
        })
        .unwrap(),
    )
    .unwrap();
    fs::write(
        root.join("hlr.toml"),
        toml::to_string(&HlrFile {
            schema: schema.clone(),
            meta: meta.clone(),
            requirements: hlr,
        })
        .unwrap(),
    )
    .unwrap();
    fs::write(
        root.join("llr.toml"),
        toml::to_string(&LlrFile {
            schema: schema.clone(),
            meta: meta.clone(),
            requirements: llr,
        })
        .unwrap(),
    )
    .unwrap();
    fs::write(
        root.join("tests.toml"),
        toml::to_string(&TestsFile {
            schema,
            meta,
            tests,
        })
        .unwrap(),
    )
    .unwrap();
    // Read back to get a real TraceFiles with the same shape production
    // would produce; leak the TempDir to keep the fixture alive during
    // the test.
    let trace =
        evidence_core::trace::read_all_trace_files(root.to_str().unwrap()).expect("read trace");
    std::mem::forget(td);
    trace
}

fn stub_entry(id: &str, uid: &str, traces_to: Vec<String>) -> HlrEntry {
    HlrEntry {
        uid: Some(uid.into()),
        ns: None,
        id: id.into(),
        title: format!("title for {id}"),
        owner: Some("tool".into()),
        scope: None,
        sort_key: None,
        category: None,
        source: None,
        description: None,
        rationale: None,
        verification_methods: vec![],
        traces_to,
        surfaces: vec![],
    }
}

fn stub_llr(id: &str, uid: &str, traces_to: Vec<String>) -> LlrEntry {
    LlrEntry {
        uid: Some(uid.into()),
        ns: None,
        id: id.into(),
        title: format!("title for {id}"),
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

fn stub_test(id: &str, uid: &str, traces_to: Vec<String>, selector: &str) -> TestEntry {
    TestEntry {
        uid: Some(uid.into()),
        ns: None,
        id: id.into(),
        title: format!("title for {id}"),
        owner: Some("tool".into()),
        sort_key: None,
        traces_to,
        description: None,
        category: None,
        test_selector: Some(selector.into()),
        test_selectors: vec![],
        source: None,
    }
}

/// TEST-025: Clean trace with every selector passing → every entry is
/// `REQ_PASS`. Byte-count the emitted diagnostics to prove one-per-
/// entry semantics (SYS + HLR + LLR + TEST = 4 diagnostics).
#[test]
fn check_source_mode_on_clean_workspace() {
    // Fix the selector to something real in this very file so the
    // resolver doesn't flag it as unresolved.
    let selector = "check_source_mode_on_clean_workspace";
    let sys = vec![stub_entry(
        "SYS-1",
        "aaaaaaaa-0000-4000-8000-000000000001",
        vec![],
    )];
    let hlr = vec![stub_entry(
        "HLR-1",
        "aaaaaaaa-0000-4000-8000-000000000002",
        vec!["aaaaaaaa-0000-4000-8000-000000000001".into()],
    )];
    let llr = vec![stub_llr(
        "LLR-1",
        "aaaaaaaa-0000-4000-8000-000000000003",
        vec!["aaaaaaaa-0000-4000-8000-000000000002".into()],
    )];
    let tests = vec![stub_test(
        "TEST-1",
        "aaaaaaaa-0000-4000-8000-000000000004",
        vec!["aaaaaaaa-0000-4000-8000-000000000003".into()],
        selector,
    )];
    let trace = synth_trace(sys, hlr, llr, tests);

    let mut outcomes = BTreeMap::new();
    outcomes.insert(
        format!("check_source_tree::{}", selector),
        TestOutcome::Passed,
    );

    let diags = build_requirement_report(
        &trace,
        &outcomes,
        // Workspace root — this test runs from the evidence crate, so
        // `..` gets us to the workspace so the selector resolver
        // succeeds.
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap(),
        &TracePolicy::default(),
    );

    assert_eq!(
        diags.len(),
        4,
        "expected exactly 4 diagnostics (one per entry): {:#?}",
        diags
    );
    for d in &diags {
        assert_eq!(d.code, "REQ_PASS", "unexpected non-PASS: {:#?}", d);
    }
}

/// TEST-026: Blanking an HLR's `traces_to` under
/// `require_hlr_sys_trace` produces `REQ_GAP` with a
/// `FixHint::AddTomlKey` that names the missing key and suggests a
/// stub value pointing at a SYS uuid. This is the mechanical-autofix
/// contract that MCP wraps.
#[test]
fn req_gap_on_blank_traces_to_has_fixhint() {
    let sys = vec![stub_entry(
        "SYS-1",
        "bbbbbbbb-0000-4000-8000-000000000001",
        vec![],
    )];
    // HLR with empty traces_to.
    let hlr = vec![stub_entry(
        "HLR-1",
        "bbbbbbbb-0000-4000-8000-000000000002",
        vec![],
    )];
    let trace = synth_trace(sys, hlr, vec![], vec![]);

    let policy = TracePolicy {
        require_hlr_sys_trace: true,
        ..TracePolicy::default()
    };

    let diags = build_requirement_report(&trace, &BTreeMap::new(), Path::new("."), &policy);

    let hlr_gap = diags
        .iter()
        .find(|d| d.code == "REQ_GAP" && d.message.contains("HLR-1"))
        .expect("HLR-1 should have a REQ_GAP");
    let hint = hlr_gap
        .fix_hint
        .as_ref()
        .expect("REQ_GAP on empty traces_to must carry a FixHint");
    match hint {
        FixHint::AddTomlKey {
            path,
            toml_path,
            key,
            value_stub,
        } => {
            assert_eq!(path.file_name().and_then(|n| n.to_str()), Some("hlr.toml"));
            assert!(toml_path.contains("requirements"));
            assert_eq!(key, "traces_to");
            assert!(
                value_stub.contains("SYS"),
                "value_stub should mention SYS-uuid: {}",
                value_stub
            );
        }
        other => panic!("expected AddTomlKey, got {:?}", other),
    }
}

/// TEST-027: A failing leaf TEST propagates `REQ_GAP` up the chain
/// with `root_cause_uid` pointing at the failing TEST. Four events
/// for one failure (TEST/LLR/HLR/SYS), agent groups client-side.
/// The shape is N events keyed by `root_cause_uid`, not a single
/// event with a derived-failure list.
#[test]
fn derived_gaps_carry_root_cause_uid() {
    let sys_uid = "cccccccc-0000-4000-8000-000000000001";
    let hlr_uid = "cccccccc-0000-4000-8000-000000000002";
    let llr_uid = "cccccccc-0000-4000-8000-000000000003";
    let test_uid = "cccccccc-0000-4000-8000-000000000004";

    // Use a known-existing selector so resolver succeeds; mark the
    // outcome as Failed.
    let selector = "derived_gaps_carry_root_cause_uid";
    let sys = vec![stub_entry("SYS-1", sys_uid, vec![])];
    let hlr = vec![stub_entry("HLR-1", hlr_uid, vec![sys_uid.into()])];
    let llr = vec![stub_llr("LLR-1", llr_uid, vec![hlr_uid.into()])];
    let tests = vec![stub_test(
        "TEST-1",
        test_uid,
        vec![llr_uid.into()],
        selector,
    )];
    let trace = synth_trace(sys, hlr, llr, tests);

    let mut outcomes = BTreeMap::new();
    outcomes.insert(
        format!("check_source_tree::{}", selector),
        TestOutcome::Failed,
    );

    let diags = build_requirement_report(
        &trace,
        &outcomes,
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap(),
        &TracePolicy::default(),
    );

    // Every entry has its own diagnostic (one-per-requirement).
    assert_eq!(diags.len(), 4);

    // Primary failure is at TEST-1 — no root_cause_uid (it *is* the
    // root).
    let test_diag = diags
        .iter()
        .find(|d| d.message.contains("TEST-1"))
        .expect("TEST-1 diag exists");
    assert_eq!(test_diag.code, "REQ_GAP");
    assert!(
        test_diag.root_cause_uid.is_none() || test_diag.root_cause_uid.as_deref() == Some(test_uid),
        "primary failure must not point elsewhere: {:?}",
        test_diag.root_cause_uid
    );

    // Derived GAPs on LLR/HLR/SYS must carry root_cause_uid = test_uid.
    for id in ["LLR-1", "HLR-1", "SYS-1"] {
        let derived = diags
            .iter()
            .find(|d| d.message.contains(id))
            .unwrap_or_else(|| panic!("{} diag exists", id));
        assert_eq!(derived.code, "REQ_GAP", "{} should be REQ_GAP", id);
        assert_eq!(
            derived.root_cause_uid.as_deref(),
            Some(test_uid),
            "{} should carry root_cause_uid pointing at TEST-1",
            id,
        );
    }
}
