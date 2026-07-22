//! CLI integration for `cargo evidence diff` (TEST-165 /
//! LLR-148): the skip-tests vs full-suite delta, the per-category
//! fixture matrix, unverifiable-category explicitness, and the
//! JSON envelope shape.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use std::fs;
use std::path::Path;

use assert_cmd::Command;
use serde_json::{Value, json};
use tempfile::TempDir;

fn cargo_evidence() -> Command {
    #[allow(deprecated)]
    Command::cargo_bin("cargo-evidence").unwrap()
}

#[path = "diff/fixture.rs"]
mod fixture;
use fixture::{llr_toml, mutate_json, write_bundle};

/// Run `cargo evidence diff --json A B` and return the parsed
/// envelope. Asserts exit 0 (diff reports, never judges).
fn diff_json(a: &Path, b: &Path) -> Value {
    let out = cargo_evidence()
        .args(["evidence", "diff", "--json"])
        .arg(a)
        .arg(b)
        .output()
        .expect("spawn");
    assert!(
        out.status.success(),
        "diff must exit 0: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).expect("diff stdout parses as JSON")
}

fn category<'a>(envelope: &'a Value, name: &str) -> &'a Value {
    envelope["categories"]
        .as_array()
        .expect("categories is an array")
        .iter()
        .find(|c| c["category"] == name)
        .unwrap_or_else(|| panic!("category {name} missing: {envelope}"))
}

// ------------------------------------------------------------------
// Acceptance scenarios
// ------------------------------------------------------------------

/// AC1: a `--skip-tests` bundle against the full suite produces a
/// clear test + verification-state delta.
#[test]
fn skip_tests_vs_full_suite_shows_test_and_state_deltas() {
    let a = TempDir::new().unwrap();
    let b = TempDir::new().unwrap();
    write_bundle(a.path(), false);
    write_bundle(b.path(), true);

    let envelope = diff_json(a.path(), b.path());
    let tests = category(&envelope, "tests");
    assert_eq!(tests["status"], "added", "tests category: {tests}");
    let details = tests["details"].as_array().unwrap();
    assert!(
        details.iter().any(|l| l
            .as_str()
            .unwrap_or("")
            .contains("present only in bundle B")),
        "tests detail must name the added evidence: {details:?}"
    );

    let states = category(&envelope, "completeness_states");
    assert_eq!(states["status"], "changed", "states category: {states}");
    let rendered = states["details"].to_string();
    assert!(
        rendered.contains("capture") || rendered.contains("reproducibility"),
        "capture/reproducibility state deltas must appear: {rendered}"
    );
}

/// AC2: one mutated artifact per assurance category, each
/// reported in its own category with the right status.
#[test]
fn changed_artifacts_each_appear_in_their_category() {
    let a = TempDir::new().unwrap();
    let b = TempDir::new().unwrap();
    write_bundle(a.path(), true);
    write_bundle(b.path(), true);

    // Trace edge, test row, coverage number, command argv,
    // standards pack, anomaly row, input, recipe field, output,
    // signature.
    fs::write(
        b.path().join("trace/llr.toml"),
        llr_toml("99999999-9999-4999-8999-999999999999"),
    )
    .unwrap();
    fs::write(
        b.path().join("tests/test_outcomes.jsonl"),
        "{\"name\":\"test_a\",\"module_path\":\"app::tests\",\"passed\":false,\"ignored\":false}\n",
    )
    .unwrap();
    mutate_json(b.path(), "coverage/coverage_summary.json", |v| {
        v["measurements"][0]["per_file"][0]["lines"]["covered"] = json!(80);
    });
    mutate_json(b.path(), "commands.json", |v| {
        v[0]["argv"] = json!(["cargo", "build"]);
    });
    mutate_json(b.path(), "compliance/app.json", |v| {
        v["standards_pack"]["version"] = json!("2");
    });
    mutate_json(b.path(), "index.json", |v| {
        v["tool_command_failures"] = json!([{
            "command_name": "cargo check",
            "exit_code": 101,
            "stderr_tail": "error",
        }]);
    });
    mutate_json(b.path(), "inputs_hashes.json", |v| {
        v["src/main.rs"] = json!("9".repeat(64));
    });
    mutate_json(b.path(), "deterministic-manifest.json", |v| {
        v["rustflags"] = json!("-C opt-level=2");
    });
    mutate_json(b.path(), "outputs_hashes.json", |v| {
        v["target/debug/app"] = json!("8".repeat(64));
    });
    fs::remove_file(b.path().join("BUNDLE.sig")).unwrap();

    let envelope = diff_json(a.path(), b.path());
    let expect_changed = [
        "trace_graph",
        "tests",
        "coverage",
        "commands",
        "objective_mappings",
        "tool_identity",
        "anomalies",
        "inputs",
        "recipe",
        "outputs",
        "integrity",
    ];
    for name in expect_changed {
        let c = category(&envelope, name);
        assert_eq!(c["status"], "changed", "category {name}: {c}");
        assert!(
            !c["details"].as_array().unwrap().is_empty(),
            "category {name} must carry details"
        );
    }

    // The reviews "fixture" is candid: reviews are corpus-side, so
    // the category reports unverifiable with the reason — there is
    // no bundle artifact to mutate.
    let reviews = category(&envelope, "reviews_approvals");
    assert_eq!(reviews["status"], "unverifiable");
    assert!(
        reviews["details"]
            .to_string()
            .contains("workspace corpus state"),
        "reviews note: {reviews}"
    );
}

/// AC3: a bundle missing coverage/ and trace/ reports those
/// categories unverifiable, and the human report never claims
/// no-changes.
#[test]
fn missing_categories_are_unverifiable_and_never_no_changes() {
    let a = TempDir::new().unwrap();
    let b = TempDir::new().unwrap();
    write_bundle(a.path(), true);
    write_bundle(b.path(), true);
    fs::remove_dir_all(b.path().join("coverage")).unwrap();
    fs::remove_dir_all(b.path().join("trace")).unwrap();

    let envelope = diff_json(a.path(), b.path());
    assert_eq!(category(&envelope, "coverage")["status"], "unverifiable");
    assert_eq!(category(&envelope, "trace_graph")["status"], "unverifiable");

    let out = cargo_evidence()
        .args(["evidence", "diff"])
        .arg(a.path())
        .arg(b.path())
        .output()
        .expect("spawn");
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).expect("utf8");
    assert!(
        !stdout.contains("(no changes)"),
        "unverifiable categories must block the no-changes marker:\n{stdout}"
    );
    assert!(stdout.contains("=== coverage === (unverifiable)"));
    assert!(stdout.contains("=== trace_graph === (unverifiable)"));
}

/// The JSON envelope carries the fixed-order categories plus the
/// legacy keys the MCP wrapper and old consumers read.
#[test]
fn json_envelope_carries_categories_and_legacy_keys() {
    let a = TempDir::new().unwrap();
    write_bundle(a.path(), true);

    let envelope = diff_json(a.path(), a.path());
    let names: Vec<&str> = envelope["categories"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["category"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "scope",
            "trace_graph",
            "tests",
            "coverage",
            "commands",
            "recipe",
            "inputs",
            "outputs",
            "objective_mappings",
            "reviews_approvals",
            "anomalies",
            "tool_identity",
            "integrity",
            "completeness_states",
            "content_hash",
        ],
        "categories must arrive in the documented fixed order"
    );
    for key in ["inputs_diff", "outputs_diff", "metadata_diff", "env_diff"] {
        assert!(
            envelope.get(key).is_some(),
            "legacy key {key} must stay populated"
        );
    }
    // Self-diff: every category equal except the permanently
    // unverifiable reviews category.
    for c in envelope["categories"].as_array().unwrap() {
        if c["category"] == "reviews_approvals" {
            assert_eq!(c["status"], "unverifiable");
        } else {
            assert_eq!(c["status"], "equal", "self-diff: {c}");
        }
    }
    // Legacy self-diff sections stay empty (golden-test contract).
    assert!(
        envelope["inputs_diff"]["added"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(envelope["env_diff"].as_array().unwrap().is_empty());
}

/// A nonexistent bundle root is an operational failure (exit 1),
/// not a report.
#[test]
fn missing_bundle_exits_nonzero() {
    let a = TempDir::new().unwrap();
    write_bundle(a.path(), true);
    let missing = a.path().join("no-such-bundle");
    let out = cargo_evidence()
        .args(["evidence", "diff"])
        .arg(&missing)
        .arg(a.path())
        .output()
        .expect("spawn");
    assert_eq!(out.status.code(), Some(1));
}

/// The `(no changes)` marker prints only when every category
/// compares equal — and since reviews/approvals are always
/// unverifiable from bundle content, even a byte-identical pair
/// must not print it.
#[test]
fn no_changes_marker_only_when_all_categories_equal() {
    let a = TempDir::new().unwrap();
    write_bundle(a.path(), true);

    let out = cargo_evidence()
        .args(["evidence", "diff"])
        .arg(a.path())
        .arg(a.path())
        .output()
        .expect("spawn");
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).expect("utf8");
    assert!(
        !stdout.contains("(no changes)"),
        "reviews are always unverifiable, so the marker must never print:\n{stdout}"
    );
    assert!(stdout.contains("=== reviews_approvals === (unverifiable)"));
    assert!(stdout.contains("=== scope === (equal)"));
    assert!(stdout.contains("=== content_hash === (equal)"));
}
