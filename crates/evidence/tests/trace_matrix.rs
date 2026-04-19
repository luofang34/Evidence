//! Integration tests for trace-link validation and the generated
//! Markdown traceability matrix.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

#[path = "helpers.rs"]
mod helpers;

use evidence::trace::{
    HlrEntry, HlrFile, LlrEntry, LlrFile, TestEntry, TestsFile, generate_traceability_matrix,
    validate_trace_links,
};

use helpers::{make_schema, make_trace_meta};

#[test]
fn test_traceability_bidirectional_matrix() {
    let hlr_uid = "550e8400-e29b-41d4-a716-446655440001";
    let llr_uid = "550e8400-e29b-41d4-a716-446655440002";
    let test_uid = "550e8400-e29b-41d4-a716-446655440003";

    let hlr_file = HlrFile {
        schema: make_schema(),
        meta: make_trace_meta(),
        requirements: vec![HlrEntry {
            uid: Some(hlr_uid.to_string()),
            ns: None,
            id: "HLR-001".to_string(),
            title: "System shall do X".to_string(),
            owner: Some("nav-kernel".to_string()),
            scope: None,
            sort_key: Some(1),
            category: None,
            source: None,
            description: None,
            rationale: None,
            verification_methods: vec!["test".to_string()],
            traces_to: vec![],
        }],
    };

    let llr_file = LlrFile {
        schema: make_schema(),
        meta: make_trace_meta(),
        requirements: vec![LlrEntry {
            uid: Some(llr_uid.to_string()),
            ns: None,
            id: "LLR-001".to_string(),
            title: "Module shall implement X".to_string(),
            owner: Some("nav-kernel".to_string()),
            sort_key: Some(1),
            traces_to: vec![hlr_uid.to_string()],
            source: None,
            modules: vec![],
            derived: false,
            description: None,
            rationale: None,
            verification_methods: vec!["test".to_string()],
            emits: vec![],
        }],
    };

    let tests_file = TestsFile {
        schema: make_schema(),
        meta: make_trace_meta(),
        tests: vec![TestEntry {
            uid: Some(test_uid.to_string()),
            ns: None,
            id: "TEST-001".to_string(),
            title: "Test that X works".to_string(),
            owner: Some("nav-kernel".to_string()),
            sort_key: Some(1),
            traces_to: vec![llr_uid.to_string()],
            description: None,
            category: None,
            test_selector: None,
            source: None,
        }],
    };

    validate_trace_links(
        &hlr_file.requirements,
        &llr_file.requirements,
        &tests_file.tests,
    )
    .expect("trace links should validate successfully");

    let matrix = generate_traceability_matrix(&hlr_file, &llr_file, &tests_file, "DOC-001");

    assert!(
        matrix.contains("HLR to LLR Traceability"),
        "Matrix should contain HLR to LLR table"
    );
    assert!(
        matrix.contains("LLR-001"),
        "HLR->LLR table should show LLR-001"
    );
    assert!(
        matrix.contains("LLR to Test Traceability"),
        "Matrix should contain LLR to Test table"
    );
    assert!(
        matrix.contains("TEST-001"),
        "LLR->Test table should show TEST-001"
    );
    assert!(
        matrix.contains("Reverse Trace: Test to LLR to HLR"),
        "Matrix must contain reverse trace table"
    );
    assert!(
        matrix.contains("End-to-End: HLR to Test Roll-Up"),
        "Matrix must contain HLR to Test roll-up table"
    );
}

#[test]
fn test_orphan_test_detection() {
    let hlr_uid = "550e8400-e29b-41d4-a716-446655440001";
    let llr_uid = "550e8400-e29b-41d4-a716-446655440002";
    let test_uid_linked = "550e8400-e29b-41d4-a716-446655440003";
    let test_uid_orphan = "550e8400-e29b-41d4-a716-446655440004";

    let hlrs = vec![HlrEntry {
        uid: Some(hlr_uid.to_string()),
        ns: None,
        id: "HLR-001".to_string(),
        title: "System requirement".to_string(),
        owner: Some("nav-kernel".to_string()),
        scope: None,
        sort_key: Some(1),
        category: None,
        source: None,
        description: None,
        rationale: None,
        verification_methods: vec!["test".to_string()],
        traces_to: vec![],
    }];

    let llrs = vec![LlrEntry {
        uid: Some(llr_uid.to_string()),
        ns: None,
        id: "LLR-001".to_string(),
        title: "Implementation requirement".to_string(),
        owner: Some("nav-kernel".to_string()),
        sort_key: Some(1),
        traces_to: vec![hlr_uid.to_string()],
        source: None,
        modules: vec![],
        derived: false,
        description: None,
        rationale: None,
        verification_methods: vec!["test".to_string()],
        emits: vec![],
    }];

    let tests = vec![
        TestEntry {
            uid: Some(test_uid_linked.to_string()),
            ns: None,
            id: "TEST-001".to_string(),
            title: "Linked test".to_string(),
            owner: Some("nav-kernel".to_string()),
            sort_key: Some(1),
            traces_to: vec![llr_uid.to_string()],
            description: None,
            category: None,
            test_selector: None,
            source: None,
        },
        TestEntry {
            uid: Some(test_uid_orphan.to_string()),
            ns: None,
            id: "TEST-ORPHAN".to_string(),
            title: "Orphan test with no LLR link".to_string(),
            owner: Some("nav-kernel".to_string()),
            sort_key: Some(2),
            traces_to: vec![],
            description: None,
            category: None,
            test_selector: None,
            source: None,
        },
    ];

    // validate_trace_links should succeed — orphans warn, they don't error.
    let result = validate_trace_links(&hlrs, &llrs, &tests);
    assert!(
        result.is_ok(),
        "Orphan tests should produce warnings, not errors: {:?}",
        result.err()
    );

    let hlr_file = HlrFile {
        schema: make_schema(),
        meta: make_trace_meta(),
        requirements: hlrs,
    };
    let llr_file = LlrFile {
        schema: make_schema(),
        meta: make_trace_meta(),
        requirements: llrs,
    };
    let tests_file = TestsFile {
        schema: make_schema(),
        meta: make_trace_meta(),
        tests,
    };

    let matrix = generate_traceability_matrix(&hlr_file, &llr_file, &tests_file, "DOC-001");

    assert!(
        matrix.contains("Orphan tests (no LLR link)"),
        "Matrix should report orphan test count"
    );
    assert!(
        matrix.contains("Orphan Tests (no LLR link)"),
        "Matrix should have orphan tests section in gaps"
    );
    assert!(
        matrix.contains("TEST-ORPHAN"),
        "Matrix should list the orphan test by ID"
    );
}
