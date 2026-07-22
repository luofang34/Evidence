//! Synthetic bundle fixture for the `cargo evidence diff` integration
//! tests — deterministic, covering every artifact the category
//! engine reads.

use std::fs;
use std::path::Path;

use serde_json::{Value, json};

// ------------------------------------------------------------------
// Synthetic bundle fixture (deterministic; covers every artifact
// the category engine reads)
// ------------------------------------------------------------------

/// Write a complete bundle into `dir`. `full_tests` selects the
/// full-suite shape; `false` the `--skip-tests` shape (no test
/// summary, no outcome rows, no captured logs).
pub(crate) fn write_bundle(dir: &Path, full_tests: bool) {
    fs::create_dir_all(dir.join("trace")).unwrap();
    fs::create_dir_all(dir.join("coverage")).unwrap();
    fs::create_dir_all(dir.join("compliance")).unwrap();

    let (test_summary, capture_state, repro_state) = if full_tests {
        (
            json!({ "total": 2, "passed": 2, "failed": 0, "ignored": 0, "filtered_out": 0 }),
            "complete",
            "complete",
        )
    } else {
        (Value::Null, "not_applicable", "not_applicable")
    };
    let index = json!({
        "schema_version": evidence_core::schema_versions::INDEX,
        "boundary_schema_version": evidence_core::schema_versions::BOUNDARY,
        "trace_schema_version": evidence_core::schema_versions::TRACE,
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
        "test_summary": test_summary,
        "dal_map": { "app": "B" },
        "resolution_policy": "locked_offline",
        "completeness": {
            "capture": capture_state,
            "graph_validity": "complete",
            "verification": "complete",
            "objective_mapping": "complete",
            "review_approval": "not_applicable",
            "integrity": "complete",
            "reproducibility": repro_state,
            "tool_qualification": "complete",
        },
    });
    fs::write(
        dir.join("index.json"),
        serde_json::to_vec_pretty(&index).unwrap(),
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
        serde_json::to_vec_pretty(&json!({ "target/debug/app": "c".repeat(64) })).unwrap(),
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
            "target_triple": "x86_64-unknown-linux-gnu",
            "tool_prerelease": false,
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        dir.join("deterministic-manifest.json"),
        serde_json::to_vec_pretty(&json!({
            "schema_version": evidence_core::schema_versions::DETERMINISTIC_MANIFEST,
            "profile": "dev",
            "rustc": "rustc 1.85.0",
            "cargo": "cargo 1.85.0",
            "target_triple": "x86_64-unknown-linux-gnu",
            "features": [],
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
        ),
    )
    .unwrap();
    fs::write(
        dir.join("trace/llr.toml"),
        llr_toml("11111111-1111-4111-8111-111111111111"),
    )
    .unwrap();
    fs::write(dir.join("trace/matrix.md"), "# matrix\n").unwrap();

    if full_tests {
        fs::create_dir_all(dir.join("tests")).unwrap();
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
    }

    fs::write(
        dir.join("coverage/coverage_summary.json"),
        serde_json::to_vec_pretty(&json!({
            "schema_version": evidence_core::schema_versions::COVERAGE,
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
        serde_json::to_vec_pretty(&json!({
            "crate_name": "app",
            "dal": "B",
            "standard": "DO-178C",
            "standard_edition": "C",
            "assurance_level": "dal_b",
            "standards_pack": { "name": "do-178c", "version": "1" },
            "schema_version": evidence_core::schema_versions::COMPLIANCE,
            "objectives": [{
                "objective_id": "A3-1", "table": "A-3", "title": "objective",
                "applicable": true, "applicability_detail": "required", "status": "met",
            }],
            "summary": {
                "total_objectives": 1, "applicable": 1, "met": 1,
                "not_met": 0, "partial": 0, "manual_review_required": 0,
            },
        }))
        .unwrap(),
    )
    .unwrap();
}

pub(crate) fn llr_toml(traces_to_uid: &str) -> String {
    let head = concat!(
        "[schema]\nversion = \"0.0.1\"\n\n[meta]\ndocument_id = \"T\"\nrevision = \"1\"\n",
        "\n[[requirements]]\nuid = \"33333333-3333-4333-8333-333333333333\"\n",
        "id = \"LLR-1\"\ntitle = \"low\"\n",
    );
    format!("{head}traces_to = [\"{traces_to_uid}\"]\n")
}

pub(crate) fn mutate_json(dir: &Path, rel: &str, mutate: impl FnOnce(&mut Value)) {
    let path = dir.join(rel);
    let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).expect("json parses");
    mutate(&mut value);
    fs::write(&path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
}
