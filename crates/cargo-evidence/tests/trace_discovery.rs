//! Integration tests for `cargo evidence trace`'s default
//! `--trace-roots` discovery (LLR-023).
//!
//! When `--trace-roots` is absent, `cmd_trace` auto-discovers
//! `./tool/trace/` first, then `./cert/trace/`, then falls back to
//! reading `cert/boundary.toml`. Explicit `--trace-roots` always
//! wins and never reaches the discovery path.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use assert_cmd::Command;
use evidence::schema_versions::TRACE;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

fn cargo_evidence() -> Command {
    #[allow(deprecated)]
    Command::cargo_bin("cargo-evidence").unwrap()
}

/// Write a minimal valid trace (hlr + llr + tests, 3 entries) into
/// `dir`. Not meant to be exhaustive — just enough to pass
/// `validate_trace_links`.
fn seed_minimal_trace(dir: &std::path::Path) {
    fs::create_dir_all(dir).unwrap();
    let hlr_uid = "11111111-2222-3333-4444-555555555555";
    let llr_uid = "22222222-3333-4444-5555-666666666666";
    let test_uid = "33333333-4444-5555-6666-777777777777";

    fs::write(
        dir.join("hlr.toml"),
        format!(
            r#"[schema]
version = "{ver}"

[meta]
document_id = "HLR"
revision = "1.0"

[[requirements]]
id = "HLR-1"
title = "Smoke HLR"
owner = "soi"
uid = "{hlr_uid}"
"#,
            ver = TRACE
        ),
    )
    .unwrap();

    fs::write(
        dir.join("llr.toml"),
        format!(
            r#"[schema]
version = "{ver}"

[meta]
document_id = "LLR"
revision = "1.0"

[[requirements]]
id = "LLR-1"
title = "Smoke LLR"
owner = "soi"
uid = "{llr_uid}"
traces_to = ["{hlr_uid}"]
verification_methods = ["test"]
"#,
            ver = TRACE
        ),
    )
    .unwrap();

    fs::write(
        dir.join("tests.toml"),
        format!(
            r#"[schema]
version = "{ver}"

[meta]
document_id = "TESTS"
revision = "1.0"

[[tests]]
id = "TEST-1"
title = "Smoke TEST"
owner = "soi"
uid = "{test_uid}"
traces_to = ["{llr_uid}"]
"#,
            ver = TRACE
        ),
    )
    .unwrap();
}

/// TEST-023: When `--trace-roots` is absent and `./tool/trace/`
/// exists, discovery picks it without any flag.
#[test]
fn trace_defaults_to_tool_trace_when_flag_absent() {
    let tmp = TempDir::new().unwrap();
    seed_minimal_trace(&tmp.path().join("tool/trace"));

    cargo_evidence()
        .env("RUST_LOG", "info")
        .arg("evidence")
        .arg("trace")
        .arg("--validate")
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "validation passed for 'tool/trace'",
        ))
        .stderr(predicate::str::contains(
            "auto-discovered trace root 'tool/trace'",
        ));
}

/// TEST-023 pair: `./cert/trace/` is the fallback when
/// `./tool/trace/` is absent. Explicit convention ordering pins
/// the behavior so an existing cert-only project still works.
#[test]
fn trace_falls_back_to_cert_trace_when_tool_trace_absent() {
    let tmp = TempDir::new().unwrap();
    seed_minimal_trace(&tmp.path().join("cert/trace"));

    cargo_evidence()
        .env("RUST_LOG", "info")
        .arg("evidence")
        .arg("trace")
        .arg("--validate")
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "validation passed for 'cert/trace'",
        ))
        .stderr(predicate::str::contains(
            "auto-discovered trace root 'cert/trace'",
        ));
}

/// Explicit `--trace-roots` always wins over discovery. Sanity
/// check: when both conventions exist, an explicit flag pointing
/// at a third path selects that path only.
#[test]
fn explicit_trace_roots_wins_over_discovery() {
    let tmp = TempDir::new().unwrap();
    seed_minimal_trace(&tmp.path().join("tool/trace"));
    seed_minimal_trace(&tmp.path().join("cert/trace"));
    seed_minimal_trace(&tmp.path().join("custom/trace"));

    cargo_evidence()
        .arg("evidence")
        .arg("trace")
        .arg("--validate")
        .arg("--trace-roots")
        .arg("custom/trace")
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "validation passed for 'custom/trace'",
        ));
}
