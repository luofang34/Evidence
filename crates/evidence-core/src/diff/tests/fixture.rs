//! Synthetic-bundle fixture for the `compare_bundles` unit tests.
//! `write_base_bundle` lays down a deterministic full bundle;
//! tests write it twice and mutate one side, so every assertion
//! keys on exactly one introduced difference.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use std::fs;
use std::path::Path;

use serde_json::{Value, json};

/// Write a complete, deterministic bundle into `dir`. Every
/// artifact the diff engine reads is present: index, hash planes,
/// commands, env, recipe manifest, SHA256SUMS, signature, trace
/// files + matrix, test outcomes + logs, coverage, compliance.
pub(crate) fn write_base_bundle(dir: &Path) {
    fs::create_dir_all(dir.join("trace")).unwrap();
    fs::create_dir_all(dir.join("tests")).unwrap();
    fs::create_dir_all(dir.join("coverage")).unwrap();
    fs::create_dir_all(dir.join("compliance")).unwrap();

    fs::write(
        dir.join("index.json"),
        serde_json::to_vec_pretty(&base_index()).unwrap(),
    )
    .unwrap();
    fs::write(
        dir.join("inputs_hashes.json"),
        serde_json::to_vec_pretty(&json!({
            "Cargo.toml": "a".repeat(64),
            "src/main.rs": "b".repeat(64),
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        dir.join("outputs_hashes.json"),
        serde_json::to_vec_pretty(&json!({
            "target/debug/app": "c".repeat(64),
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        dir.join("commands.json"),
        serde_json::to_vec_pretty(&json!([{
            "argv": ["cargo", "test", "--workspace"],
            "cwd": "/workspace",
            "exit_code": 0,
            "stdout_path": "tests/cargo_test_stdout.txt",
            "stderr_path": "tests/cargo_test_stderr.txt",
        }]))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        dir.join("env.json"),
        serde_json::to_vec_pretty(&json!({
            "profile": "dev",
            "rustc": "rustc 1.85.0",
            "cargo": "cargo 1.85.0",
            "llvm_version": "19.1",
            "target_triple": "x86_64-unknown-linux-gnu",
            "tool_prerelease": false,
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        dir.join("deterministic-manifest.json"),
        serde_json::to_vec_pretty(&json!({
            "schema_version": crate::schema_versions::DETERMINISTIC_MANIFEST,
            "profile": "dev",
            "rustc": "rustc 1.85.0",
            "cargo": "cargo 1.85.0",
            "llvm_version": "19.1",
            "cargo_lock_hash": "1".repeat(64),
            "rust_toolchain_toml": null,
            "rustflags": null,
            "target_triple": "x86_64-unknown-linux-gnu",
            "features": [],
            "locked_graph_hash": "2".repeat(64),
            "command_recipe_hash": "3".repeat(64),
            "inputs_hash": "4".repeat(64),
            "resolution_policy": "locked_offline",
            "git_sha": "f".repeat(40),
            "git_branch": "main",
            "git_dirty": false,
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(dir.join("SHA256SUMS"), "dummy\n").unwrap();
    fs::write(dir.join("BUNDLE.sig"), "sig\n").unwrap();

    fs::write(
        dir.join("trace/hlr.toml"),
        concat!(
            "[schema]\nversion = \"0.0.1\"\n\n[meta]\ndocument_id = \"T\"\nrevision = \"1\"\n",
            "\n[[requirements]]\nuid = \"11111111-1111-4111-8111-111111111111\"\n",
            "id = \"HLR-1\"\ntitle = \"first\"\n",
            "\n[[requirements]]\nuid = \"22222222-2222-4222-8222-222222222222\"\n",
            "id = \"HLR-2\"\ntitle = \"second\"\n",
        ),
    )
    .unwrap();
    fs::write(
        dir.join("trace/llr.toml"),
        llr_toml("11111111-1111-4111-8111-111111111111"),
    )
    .unwrap();
    fs::write(
        dir.join("trace/tests.toml"),
        concat!(
            "[schema]\nversion = \"0.0.1\"\n\n[meta]\ndocument_id = \"T\"\nrevision = \"1\"\n",
            "\n[[tests]]\nuid = \"44444444-4444-4444-8444-444444444444\"\n",
            "id = \"TEST-1\"\ntitle = \"case\"\n",
            "traces_to = [\"33333333-3333-4333-8333-333333333333\"]\n",
        ),
    )
    .unwrap();
    fs::write(dir.join("trace/matrix.md"), "# matrix\n").unwrap();

    fs::write(
        dir.join("tests/test_outcomes.jsonl"),
        concat!(
            "{\"name\":\"test_a\",\"module_path\":\"app::tests\",\"passed\":true,\"ignored\":false}\n",
            "{\"name\":\"test_b\",\"module_path\":\"app::tests\",\"passed\":true,\"ignored\":false}\n",
        ),
    )
    .unwrap();
    fs::write(dir.join("tests/cargo_test_stdout.txt"), "ok\n").unwrap();
    fs::write(dir.join("tests/cargo_test_stderr.txt"), "").unwrap();

    fs::write(
        dir.join("coverage/coverage_summary.json"),
        serde_json::to_vec_pretty(&json!({
            "schema_version": crate::schema_versions::COVERAGE,
            "measurements": [{
                "level": "statement",
                "engine": "llvm-cov",
                "engine_version": "0.8.5",
                "per_file": [{
                    "path": "src/main.rs",
                    "lines": { "covered": 90, "total": 100 },
                    "decisions": [],
                    "conditions": [],
                }],
            }],
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(dir.join("coverage/lcov.info"), "TN:\n").unwrap();

    fs::write(
        dir.join("compliance/app.json"),
        serde_json::to_vec_pretty(&base_compliance_report()).unwrap(),
    )
    .unwrap();
}

/// The base `index.json` as raw JSON so tests can mutate fields
/// without rebuilding the typed struct.
fn base_index() -> Value {
    json!({
        "schema_version": crate::schema_versions::INDEX,
        "boundary_schema_version": crate::schema_versions::BOUNDARY,
        "trace_schema_version": crate::schema_versions::TRACE,
        "profile": "dev",
        "timestamp_rfc3339": "2026-01-01T00:00:00Z",
        "git_sha": "f".repeat(40),
        "git_branch": "main",
        "git_dirty": false,
        "engine_crate_version": "0.1.0",
        "engine_git_sha": "e".repeat(40),
        "engine_build_source": "git",
        "inputs_hashes_file": "inputs_hashes.json",
        "outputs_hashes_file": "outputs_hashes.json",
        "commands_file": "commands.json",
        "env_fingerprint_file": "env.json",
        "trace_roots": ["cert/trace"],
        "trace_outputs": ["trace/matrix.md"],
        "bundle_complete": true,
        "content_hash": "c".repeat(64),
        "recipe_hash": "d".repeat(64),
        "test_summary": { "total": 2, "passed": 2, "failed": 0, "ignored": 0, "filtered_out": 0 },
        "dal_map": { "app": "B" },
        "resolution_policy": "locked_offline",
        "completeness": {
            "capture": "complete",
            "graph_validity": "complete",
            "verification": "complete",
            "objective_mapping": "complete",
            "review_approval": "not_applicable",
            "integrity": "complete",
            "reproducibility": "complete",
            "tool_qualification": "complete",
        },
    })
}

/// The base compliance report for crate `app`.
fn base_compliance_report() -> Value {
    json!({
        "crate_name": "app",
        "dal": "B",
        "standard": "DO-178C",
        "standard_edition": "C",
        "assurance_level": "dal_b",
        "standards_pack": { "name": "do-178c", "version": "1" },
        "schema_version": crate::schema_versions::COMPLIANCE,
        "objectives": [{
            "objective_id": "A3-1",
            "table": "A-3",
            "title": "objective",
            "applicable": true,
            "applicability_detail": "required",
            "status": "met",
        }],
        "summary": {
            "total_objectives": 1,
            "applicable": 1,
            "met": 1,
            "not_met": 0,
            "partial": 0,
            "manual_review_required": 0,
        },
    })
}

/// `trace/llr.toml` with the entry's `traces_to` pointing at the
/// given HLR uid — the trace-edge mutation knob.
pub(crate) fn llr_toml(traces_to_uid: &str) -> String {
    let head = concat!(
        "[schema]\nversion = \"0.0.1\"\n\n[meta]\ndocument_id = \"T\"\nrevision = \"1\"\n",
        "\n[[requirements]]\nuid = \"33333333-3333-4333-8333-333333333333\"\n",
        "id = \"LLR-1\"\ntitle = \"low\"\n",
    );
    format!("{head}traces_to = [\"{traces_to_uid}\"]\n")
}

/// Read `index.json` from `dir` as a mutable JSON value, apply
/// `mutate`, and write it back.
pub(crate) fn mutate_index(dir: &Path, mutate: impl FnOnce(&mut Value)) {
    let path = dir.join("index.json");
    let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).expect("index parses");
    mutate(&mut value);
    fs::write(&path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
}

/// Read a JSON artifact relative to `dir`, apply `mutate`, write
/// it back. Used for compliance reports and hash planes.
pub(crate) fn mutate_json(dir: &Path, rel: &str, mutate: impl FnOnce(&mut Value)) {
    let path = dir.join(rel);
    let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).expect("json parses");
    mutate(&mut value);
    fs::write(&path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
}
