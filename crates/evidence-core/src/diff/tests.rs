//! Tests for `compare_bundles` — per-category statuses across the
//! full assurance surface (TEST-164). Each test writes the
//! deterministic base bundle twice and mutates one artifact, so the
//! asserted category status keys on exactly one introduced
//! difference.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use super::*;
use std::fs;
use tempfile::TempDir;

#[path = "tests/fixture.rs"]
mod fixture;
use fixture::{llr_toml, mutate_index, mutate_json, write_base_bundle};

/// Write the base bundle into two fresh tempdirs.
fn pair() -> (TempDir, TempDir) {
    let a = TempDir::new().expect("tempdir a");
    let b = TempDir::new().expect("tempdir b");
    write_base_bundle(a.path());
    write_base_bundle(b.path());
    (a, b)
}

/// Compare the pair and return the categories.
fn diff_of(a: &TempDir, b: &TempDir) -> Vec<CategoryDiff> {
    compare_bundles(a.path(), b.path()).expect("compare succeeds")
}

fn category<'a>(diffs: &'a [CategoryDiff], name: &str) -> &'a CategoryDiff {
    diffs
        .iter()
        .find(|d| d.category == name)
        .unwrap_or_else(|| panic!("category {name} missing from {diffs:?}"))
}

/// A byte-identical pair compares Equal everywhere except
/// `reviews_approvals`, which is Unverifiable by design — the
/// report never claims total equality while reviews remain
/// unexaminable.
#[test]
fn identical_bundles_compare_equal_except_reviews() {
    let (a, b) = pair();
    let diffs = diff_of(&a, &b);
    assert_eq!(
        diffs.iter().map(|d| d.category).collect::<Vec<_>>(),
        CATEGORY_ORDER,
        "categories must come back in the documented fixed order"
    );
    for d in &diffs {
        if d.category == "reviews_approvals" {
            assert_eq!(d.status, DiffCategoryStatus::Unverifiable);
            assert!(
                d.details
                    .iter()
                    .any(|l| l.contains("workspace corpus state")),
                "reviews note must explain why: {d:?}"
            );
        } else {
            assert_eq!(
                d.status,
                DiffCategoryStatus::Equal,
                "identical bundles: {} must be equal, got {d:?}",
                d.category
            );
        }
    }
}

#[test]
fn scope_change_is_changed() {
    let (a, b) = pair();
    mutate_index(b.path(), |v| {
        v["profile"] = serde_json::json!("cert");
    });
    let diffs = diff_of(&a, &b);
    let d = category(&diffs, "scope");
    assert_eq!(d.status, DiffCategoryStatus::Changed);
    assert!(
        d.details
            .iter()
            .any(|l| l.contains("profile") && l.contains("dev") && l.contains("cert")),
        "scope detail must name the profile change: {d:?}"
    );
}

#[test]
fn trace_entry_change_is_changed() {
    let (a, b) = pair();
    // Rewrite llr.toml in B with the edge retargeted to the second HLR.
    fs::write(
        b.path().join("trace/llr.toml"),
        llr_toml("22222222-2222-4222-8222-222222222222"),
    )
    .expect("rewrite llr.toml");
    let diffs = diff_of(&a, &b);
    let d = category(&diffs, "trace_graph");
    assert_eq!(d.status, DiffCategoryStatus::Changed);
    assert!(
        d.details
            .iter()
            .any(|l| l.contains("llr.toml") && l.contains("LLR-1")),
        "trace detail must name the changed entry: {d:?}"
    );
}

#[test]
fn missing_trace_dir_is_unverifiable() {
    let (a, b) = pair();
    fs::remove_dir_all(b.path().join("trace")).expect("remove trace dir");
    let diffs = diff_of(&a, &b);
    let d = category(&diffs, "trace_graph");
    assert_eq!(d.status, DiffCategoryStatus::Unverifiable);
    assert!(
        d.details.iter().any(|l| l.contains("bundle B")),
        "reason must name the side: {d:?}"
    );
}

#[test]
fn test_row_flip_and_added_rows_are_changed() {
    let (a, b) = pair();
    // Flip test_a to failed and add test_c in B.
    fs::write(
        b.path().join("tests/test_outcomes.jsonl"),
        concat!(
            "{\"name\":\"test_a\",\"module_path\":\"app::tests\",\"passed\":false,\"ignored\":false}\n",
            "{\"name\":\"test_b\",\"module_path\":\"app::tests\",\"passed\":true,\"ignored\":false}\n",
            "{\"name\":\"test_c\",\"module_path\":\"app::tests\",\"passed\":true,\"ignored\":false}\n",
        ),
    )
    .expect("rewrite outcomes");
    mutate_index(b.path(), |v| {
        v["test_summary"]["passed"] = serde_json::json!(2);
        v["test_summary"]["failed"] = serde_json::json!(1);
        v["test_summary"]["total"] = serde_json::json!(3);
    });
    let diffs = diff_of(&a, &b);
    let d = category(&diffs, "tests");
    assert_eq!(d.status, DiffCategoryStatus::Changed);
    assert!(
        d.details.iter().any(|l| l.contains("app::tests::test_a")
            && l.contains("passed")
            && l.contains("failed")),
        "flip must be reported: {d:?}"
    );
    assert!(
        d.details.iter().any(|l| l.contains("app::tests::test_c")),
        "added row must be reported: {d:?}"
    );
    assert!(
        d.details.iter().any(|l| l.contains("test_summary.failed")),
        "summary delta must be reported: {d:?}"
    );
}

/// `duration_ms` and `failure_message` are excluded from row
/// equality by design — pass/fail and presence only.
#[test]
fn duration_and_failure_message_drift_compares_equal() {
    let (a, b) = pair();
    fs::write(
        b.path().join("tests/test_outcomes.jsonl"),
        concat!(
            "{\"name\":\"test_a\",\"module_path\":\"app::tests\",\"passed\":true,\"ignored\":false,",
            "\"duration_ms\":42,\"failure_message\":\"noise\"}\n",
            "{\"name\":\"test_b\",\"module_path\":\"app::tests\",\"passed\":true,\"ignored\":false,",
            "\"duration_ms\":7}\n",
        ),
    )
    .expect("rewrite outcomes");
    let diffs = diff_of(&a, &b);
    let d = category(&diffs, "tests");
    assert_eq!(
        d.status,
        DiffCategoryStatus::Equal,
        "timing/message drift must not flip equality: {d:?}"
    );
}

/// The acceptance scenario: a `--skip-tests` bundle against a
/// full-suite bundle reports the tests category as Added with the
/// present side's evidence summarized.
#[test]
fn skip_tests_pair_reports_tests_added() {
    let (a, b) = pair();
    // Strip every test artifact from A — the skip-tests side.
    mutate_index(a.path(), |v| {
        v.as_object_mut().expect("object").remove("test_summary");
    });
    fs::remove_dir_all(a.path().join("tests")).expect("remove tests dir");
    let diffs = diff_of(&a, &b);
    let d = category(&diffs, "tests");
    assert_eq!(d.status, DiffCategoryStatus::Added);
    assert!(
        d.details
            .iter()
            .any(|l| l.contains("present only in bundle B") && l.contains("outcome row")),
        "added detail must summarize B's evidence: {d:?}"
    );
}

#[test]
fn coverage_aggregate_change_is_changed() {
    let (a, b) = pair();
    mutate_json(b.path(), "coverage/coverage_summary.json", |v| {
        v["measurements"][0]["per_file"][0]["lines"]["covered"] = serde_json::json!(80);
    });
    let diffs = diff_of(&a, &b);
    let d = category(&diffs, "coverage");
    assert_eq!(d.status, DiffCategoryStatus::Changed);
    assert!(
        d.details
            .iter()
            .any(|l| l.contains("statement line coverage")
                && l.contains("90/100")
                && l.contains("80/100")),
        "aggregate delta must be reported: {d:?}"
    );
}

#[test]
fn command_row_changes_are_changed() {
    let (a, b) = pair();
    mutate_json(b.path(), "commands.json", |v| {
        v[0]["argv"] = serde_json::json!(["cargo", "test", "--workspace", "--release"]);
        v[0]["exit_code"] = serde_json::json!(101);
    });
    let diffs = diff_of(&a, &b);
    let d = category(&diffs, "commands");
    assert_eq!(d.status, DiffCategoryStatus::Changed);
    assert!(
        d.details
            .iter()
            .any(|l| l.contains("argv") && l.contains("--release")),
        "argv change must be reported: {d:?}"
    );
    assert!(
        d.details
            .iter()
            .any(|l| l.contains("exit_code") && l.contains("101")),
        "exit code change must be reported: {d:?}"
    );
}

#[test]
fn recipe_field_change_names_the_field() {
    let (a, b) = pair();
    mutate_json(b.path(), "deterministic-manifest.json", |v| {
        v["target_triple"] = serde_json::json!("aarch64-apple-darwin");
    });
    let diffs = diff_of(&a, &b);
    let d = category(&diffs, "recipe");
    assert_eq!(d.status, DiffCategoryStatus::Changed);
    assert!(
        d.details.iter().any(|l| l.contains("target_triple")),
        "changed recipe field must be named: {d:?}"
    );
}

#[test]
fn input_and_output_plane_changes_are_changed() {
    let (a, b) = pair();
    mutate_json(b.path(), "inputs_hashes.json", |v| {
        let map = v.as_object_mut().expect("object");
        map.insert("src/main.rs".to_string(), serde_json::json!("9".repeat(64)));
        map.insert("src/new.rs".to_string(), serde_json::json!("8".repeat(64)));
    });
    mutate_json(b.path(), "outputs_hashes.json", |v| {
        v.as_object_mut()
            .expect("object")
            .remove("target/debug/app");
    });
    let diffs = diff_of(&a, &b);
    let inputs = category(&diffs, "inputs");
    assert_eq!(inputs.status, DiffCategoryStatus::Changed);
    assert!(
        inputs.details.iter().any(|l| l == "~ src/main.rs"),
        "changed input: {inputs:?}"
    );
    assert!(
        inputs.details.iter().any(|l| l == "+ src/new.rs"),
        "added input: {inputs:?}"
    );
    let outputs = category(&diffs, "outputs");
    assert_eq!(outputs.status, DiffCategoryStatus::Changed);
    assert!(
        outputs.details.iter().any(|l| l == "- target/debug/app"),
        "removed output: {outputs:?}"
    );
}

#[test]
fn standards_pack_change_is_changed() {
    let (a, b) = pair();
    mutate_json(b.path(), "compliance/app.json", |v| {
        v["standards_pack"]["version"] = serde_json::json!("2");
    });
    let diffs = diff_of(&a, &b);
    let mappings = category(&diffs, "objective_mappings");
    assert_eq!(mappings.status, DiffCategoryStatus::Changed);
    assert!(
        mappings
            .details
            .iter()
            .any(|l| l.contains("standards_pack")),
        "pack identity must be reported under objective_mappings: {mappings:?}"
    );
    let identity = category(&diffs, "tool_identity");
    assert_eq!(identity.status, DiffCategoryStatus::Changed);
    assert!(
        identity
            .details
            .iter()
            .any(|l| l.contains("standards_pack")),
        "pack identity must be reported under tool_identity: {identity:?}"
    );
}

#[test]
fn anomalies_row_added_is_changed() {
    let (a, b) = pair();
    mutate_index(b.path(), |v| {
        v["tool_command_failures"] = serde_json::json!([{
            "command_name": "cargo test --workspace",
            "exit_code": 101,
            "stderr_tail": "error: test failed",
        }]);
    });
    let diffs = diff_of(&a, &b);
    let d = category(&diffs, "anomalies");
    assert_eq!(d.status, DiffCategoryStatus::Changed);
    assert!(
        d.details
            .iter()
            .any(|l| l.contains("cargo test --workspace") && l.contains("101")),
        "added failure row must be reported: {d:?}"
    );
}

#[test]
fn integrity_and_content_hash_changes_are_changed() {
    let (a, b) = pair();
    fs::remove_file(b.path().join("BUNDLE.sig")).expect("remove signature");
    mutate_index(b.path(), |v| {
        v["content_hash"] = serde_json::json!("0".repeat(64));
    });
    let diffs = diff_of(&a, &b);
    let integrity = category(&diffs, "integrity");
    assert_eq!(integrity.status, DiffCategoryStatus::Changed);
    assert!(
        integrity
            .details
            .iter()
            .any(|l| l.contains("BUNDLE.sig") && l.contains("present") && l.contains("absent")),
        "signature presence change: {integrity:?}"
    );
    let hash = category(&diffs, "content_hash");
    assert_eq!(hash.status, DiffCategoryStatus::Changed);
    assert!(
        hash.details.iter().any(|l| l.contains("content_hash")),
        "hash change detail: {hash:?}"
    );
}

#[test]
fn completeness_states_delta_is_changed() {
    let (a, b) = pair();
    mutate_index(b.path(), |v| {
        v["completeness"]["verification"] = serde_json::json!("unverifiable");
        v["completeness"]["reproducibility"] = serde_json::json!("not_applicable");
    });
    let diffs = diff_of(&a, &b);
    let d = category(&diffs, "completeness_states");
    assert_eq!(d.status, DiffCategoryStatus::Changed);
    assert!(
        d.details.iter().any(|l| l.contains("verification")
            && l.contains("complete")
            && l.contains("unverifiable")),
        "verification state delta: {d:?}"
    );
    assert!(
        d.details.iter().any(|l| l.contains("reproducibility")),
        "reproducibility state delta: {d:?}"
    );
}

#[test]
fn missing_bundle_dir_is_an_error() {
    let (a, _b) = pair();
    let missing = a.path().join("no-such-bundle");
    let err = compare_bundles(&missing, a.path()).expect_err("missing dir must error");
    assert!(matches!(err, DiffError::BundleNotFound(_)));
    let err = compare_bundles(a.path(), &missing).expect_err("missing dir must error");
    assert!(matches!(err, DiffError::BundleNotFound(_)));
}

/// Uncompared categories are explicit: a bundle missing coverage/
/// and trace/ reports those categories Unverifiable, and the
/// report as a whole cannot read as all-equal.
#[test]
fn missing_planes_are_unverifiable_not_equal() {
    let (a, b) = pair();
    fs::remove_dir_all(b.path().join("coverage")).expect("remove coverage dir");
    fs::remove_dir_all(b.path().join("trace")).expect("remove trace dir");
    let diffs = diff_of(&a, &b);
    assert_eq!(
        category(&diffs, "coverage").status,
        DiffCategoryStatus::Unverifiable
    );
    assert_eq!(
        category(&diffs, "trace_graph").status,
        DiffCategoryStatus::Unverifiable
    );
    assert!(
        diffs.iter().any(|d| d.status != DiffCategoryStatus::Equal),
        "the report must not read as no-changes"
    );
}

/// Detail lines are sorted inside every category, so two runs of
/// the same comparison produce byte-identical reports.
#[test]
fn details_are_sorted_deterministically() {
    let (a, b) = pair();
    mutate_json(b.path(), "inputs_hashes.json", |v| {
        let map = v.as_object_mut().expect("object");
        map.insert("zzz.rs".to_string(), serde_json::json!("8".repeat(64)));
        map.insert("aaa.rs".to_string(), serde_json::json!("7".repeat(64)));
        map.remove("Cargo.toml");
        map.insert("src/main.rs".to_string(), serde_json::json!("9".repeat(64)));
    });
    let first = diff_of(&a, &b);
    let second = diff_of(&a, &b);
    assert_eq!(first, second, "the comparison must be deterministic");
    for d in &first {
        let mut sorted = d.details.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(d.details, sorted, "{} details must be sorted", d.category);
    }
}
