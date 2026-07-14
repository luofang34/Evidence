//! Unit tests for the nextest libtest-json-plus parser. Fixture lines
//! are real events captured from `cargo nextest run --message-format
//! libtest-json-plus`.

#![allow(clippy::expect_used, clippy::panic)]

use super::*;

// A small fixture: one lib suite with a unit test, a failed test, and
// an ignored test, plus an integration suite with a bare test.
fn stream() -> String {
    [
        r#"{"type":"suite","event":"started","test_count":3,"nextest":{"crate":"evidence-core","test_binary":"evidence_core","kind":"lib"}}"#,
        r#"{"type":"test","event":"ok","name":"evidence-core::evidence_core$bundle::input_scope::tests::resolves_package_name_to_manifest_dir","exec_time":0.01}"#,
        r#"{"type":"test","event":"failed","name":"evidence-core::evidence_core$verify::foo::bar_fails","stdout":"assertion failed: 1 == 2","exec_time":0.02}"#,
        r#"{"type":"test","event":"ignored","name":"evidence-core::evidence_core$slow::heavy_case"}"#,
        r#"{"type":"suite","event":"failed","passed":1,"failed":1,"ignored":1,"measured":0,"filtered_out":2,"exec_time":0.5,"nextest":{"crate":"evidence-core","test_binary":"evidence_core","kind":"lib"}}"#,
        r#"{"type":"suite","event":"started","test_count":1,"nextest":{"crate":"cargo-evidence","test_binary":"input_baseline","kind":"test"}}"#,
        r#"{"type":"test","event":"ok","name":"cargo-evidence::input_baseline$captured_baseline_agrees_with_independent_enumeration","exec_time":0.9}"#,
        r#"{"type":"suite","event":"ok","passed":1,"failed":0,"ignored":0,"measured":0,"filtered_out":0,"exec_time":0.9,"nextest":{"crate":"cargo-evidence","test_binary":"input_baseline","kind":"test"}}"#,
    ]
    .join("\n")
}

#[test]
fn unit_test_identity_matches_selector_format() {
    let run = parse_nextest_libtest_json(&stream());
    let rec = run
        .records
        .iter()
        .find(|r| r.name == "resolves_package_name_to_manifest_dir")
        .expect("record present");
    assert_eq!(rec.package, "evidence-core");
    assert_eq!(rec.binary, "evidence_core");
    assert_eq!(rec.harness, "libtest");
    assert_eq!(rec.module_path, "evidence_core::bundle::input_scope::tests");
    // {module_path}::{name} == the test_selector the trace uses.
    assert_eq!(
        format!("{}::{}", rec.module_path, rec.name),
        "evidence_core::bundle::input_scope::tests::resolves_package_name_to_manifest_dir"
    );
    assert!(rec.passed && !rec.ignored);
}

#[test]
fn hyphenated_bin_binary_is_normalized_to_underscore() {
    // A `cargo-evidence` bin unit test: nextest reports the target name
    // verbatim, but the key must use the underscored identifier so it
    // matches the libtest-text capture (`check`) and the module-path
    // convention.
    let line = r#"{"type":"test","event":"ok","name":"cargo-evidence::cargo-evidence$cli::keygen::tests::rotate_overwrites_and_logs"}"#;
    let run = parse_nextest_libtest_json(line);
    let rec = run.records.first().expect("record");
    assert_eq!(rec.package, "cargo-evidence");
    assert_eq!(rec.binary, "cargo_evidence");
    assert_eq!(rec.module_path, "cargo_evidence::cli::keygen::tests");
    assert_eq!(
        format!("{}::{}", rec.module_path, rec.name),
        "cargo_evidence::cli::keygen::tests::rotate_overwrites_and_logs"
    );
}

#[test]
fn integration_test_bare_fn_identity() {
    let run = parse_nextest_libtest_json(&stream());
    let rec = run
        .records
        .iter()
        .find(|r| r.binary == "input_baseline")
        .expect("integration record");
    assert_eq!(rec.package, "cargo-evidence");
    assert_eq!(rec.module_path, "input_baseline");
    assert_eq!(
        rec.name,
        "captured_baseline_agrees_with_independent_enumeration"
    );
    assert_eq!(
        format!("{}::{}", rec.module_path, rec.name),
        "input_baseline::captured_baseline_agrees_with_independent_enumeration"
    );
}

#[test]
fn failed_test_captures_message_and_not_populate_duration() {
    let run = parse_nextest_libtest_json(&stream());
    let rec = run
        .records
        .iter()
        .find(|r| r.name == "bar_fails")
        .expect("failed record");
    assert!(!rec.passed && !rec.ignored);
    assert_eq!(
        rec.failure_message.as_deref(),
        Some("assertion failed: 1 == 2")
    );
    // Determinism: exec_time is never turned into a duration.
    assert_eq!(rec.duration_ms, None);
}

#[test]
fn ignored_test_recorded() {
    let run = parse_nextest_libtest_json(&stream());
    let rec = run
        .records
        .iter()
        .find(|r| r.name == "heavy_case")
        .expect("ignored record");
    assert!(rec.ignored && !rec.passed);
}

#[test]
fn summary_aggregates_across_suites() {
    let run = parse_nextest_libtest_json(&stream());
    // Suite 1: 1 passed, 1 failed, 1 ignored, 2 filtered. Suite 2: 1 passed.
    assert_eq!(run.summary.passed, 2);
    assert_eq!(run.summary.failed, 1);
    assert_eq!(run.summary.ignored, 1);
    assert_eq!(run.summary.filtered_out, 2);
    assert_eq!(run.summary.total, 6);
    // Rows = passed + failed + ignored (filtered_out has no test event).
    assert_eq!(run.records.len(), 4);
}

#[test]
fn records_are_sorted_for_determinism() {
    let run = parse_nextest_libtest_json(&stream());
    let keys: Vec<String> = run
        .records
        .iter()
        .map(|r| format!("{}::{}", r.module_path, r.name))
        .collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(
        keys, sorted,
        "records must be emitted in sorted identity order"
    );
}

#[test]
fn malformed_and_unknown_lines_are_skipped() {
    let stream = [
        "not json at all",
        r#"{"type":"info","event":"whatever"}"#,
        r#"{"type":"test","event":"started","name":"x::y$z"}"#,
        r#"{"type":"suite","event":"ok","passed":0,"failed":0,"ignored":0,"filtered_out":0,"nextest":{"crate":"x","test_binary":"y","kind":"lib"}}"#,
    ]
    .join("\n");
    let run = parse_nextest_libtest_json(&stream);
    assert!(run.records.is_empty());
    assert_eq!(run.summary.total, 0);
}
