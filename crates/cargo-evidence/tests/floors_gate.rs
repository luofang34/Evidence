//! Integration tests for `cargo evidence floors` (PR #48 /
//! LLR-036 / TEST-036).
//!
//! Two scenarios:
//!
//! 1. The committed `cert/floors.toml` is satisfied by the current
//!    tree — exit 0, every row reports `status = "pass"`.
//! 2. A tampered floors.toml (bump `diagnostic_codes` above the
//!    library's compiled-in RULES count) fires the gate with
//!    `FLOORS_BELOW_MIN` and exit 2.
//!
//! The tampered case uses the `--config` flag to point at a tempdir
//! fixture; the committed file stays untouched.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use std::path::PathBuf;

use assert_cmd::Command;
use serde_json::Value;
use tempfile::TempDir;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

fn cargo_evidence() -> Command {
    #[allow(deprecated)]
    Command::cargo_bin("cargo-evidence").unwrap()
}

/// Happy path: the committed cert/floors.toml is satisfied by the
/// current tree. Every row passes; exit 0.
#[test]
fn floors_gate_passes_on_committed_state() {
    let out = cargo_evidence()
        .current_dir(workspace_root())
        .args(["evidence", "floors", "--json"])
        .output()
        .expect("spawn");
    assert!(
        out.status.success(),
        "committed floors.toml must pass; stdout={}\nstderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    let rows: Vec<Value> = serde_json::from_str(&stdout).expect("parses");
    assert!(!rows.is_empty(), "expected at least one floor row");

    let fails: Vec<&Value> = rows.iter().filter(|r| r["status"] == "fail").collect();
    assert!(fails.is_empty(), "unexpected failing rows: {:?}", fails);
}

/// Tampered floor: bump `diagnostic_codes` to 999 (far above RULES
/// count). The gate must fire FLOORS_BELOW_MIN naming the dimension
/// and exit 2.
#[test]
fn floors_gate_fires_on_below_min_floor() {
    let tmp = TempDir::new().expect("tempdir");
    let tampered = tmp.path().join("floors.toml");
    std::fs::write(
        &tampered,
        r#"[floors]
diagnostic_codes = 999
"#,
    )
    .expect("write tampered floors.toml");

    let out = cargo_evidence()
        .current_dir(workspace_root())
        .args(["evidence", "floors", "--config"])
        .arg(&tampered)
        .output()
        .expect("spawn");
    assert_eq!(
        out.status.code(),
        Some(2),
        "tampered floor must fail with exit 2; stdout={}\nstderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    assert!(
        stdout.contains("FLOORS_BELOW_MIN"),
        "expected FLOORS_BELOW_MIN in output; got:\n{}",
        stdout
    );
    assert!(
        stdout.contains("diagnostic_codes"),
        "expected dimension name in output; got:\n{}",
        stdout
    );
}

/// Downstream UX: a project without `cert/floors.toml` must not see
/// a scary error. The CLI emits an info message on stderr and exits
/// 0 — the user opts in by creating the file.
#[test]
fn missing_floors_toml_is_a_friendly_skip_not_a_hard_error() {
    let tmp = TempDir::new().expect("tempdir");
    let missing = tmp.path().join("floors-does-not-exist.toml");
    let out = cargo_evidence()
        .current_dir(workspace_root())
        .args(["evidence", "floors", "--config"])
        .arg(&missing)
        .output()
        .expect("spawn");
    assert!(
        out.status.success(),
        "missing config must be a friendly skip (exit 0); stdout={}\nstderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("no floors config") || stderr.contains("not configured"),
        "expected a friendly info message on stderr; got:\n{}",
        stderr
    );
}

/// Malformed TOML must fail hard (exit 1) — a typo'd path silently
/// passing is a worse outcome than a loud error.
#[test]
fn malformed_floors_toml_is_a_hard_error() {
    let tmp = TempDir::new().expect("tempdir");
    let bad = tmp.path().join("floors.toml");
    std::fs::write(&bad, "this is not = valid {{{").expect("write");
    let out = cargo_evidence()
        .current_dir(workspace_root())
        .args(["evidence", "floors", "--config"])
        .arg(&bad)
        .output()
        .expect("spawn");
    assert_eq!(
        out.status.code(),
        Some(1),
        "malformed TOML must exit 1; stdout={}\nstderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
}

/// Bijection: the set of `[per_crate.<name>]` keys in
/// `cert/floors.toml` must equal `scope.in_scope` in
/// `cert/boundary.toml`. Adding a crate to the workspace requires
/// adding its per-crate floor row in the same PR; conversely, an
/// orphan `per_crate.<name>` entry for a removed crate must be
/// cleaned up. Same contract pattern as PR #47's
/// `every_code_is_claimed_by_an_llr` — the bijection is the whole
/// point of committing per-crate floors.
#[test]
fn per_crate_floors_match_boundary_in_scope() {
    let root = workspace_root();
    let floors = evidence::FloorsConfig::load(&root.join("cert").join("floors.toml"))
        .expect("load floors.toml");
    let boundary = evidence::BoundaryConfig::load(&root.join("cert").join("boundary.toml"))
        .expect("load boundary.toml");

    let in_scope: std::collections::BTreeSet<String> =
        boundary.scope.in_scope.iter().cloned().collect();
    let declared: std::collections::BTreeSet<String> = floors.per_crate.keys().cloned().collect();

    let only_in_boundary: Vec<&String> = in_scope.difference(&declared).collect();
    let only_in_floors: Vec<&String> = declared.difference(&in_scope).collect();

    assert!(
        only_in_boundary.is_empty() && only_in_floors.is_empty(),
        "cert/floors.toml [per_crate.*] must match cert/boundary.toml scope.in_scope\n\
         in boundary but missing from floors.toml: {:?}\n\
         in floors.toml but missing from boundary.toml: {:?}",
        only_in_boundary,
        only_in_floors
    );
}

/// JSON mode is a valid JSON array and round-trips through serde.
#[test]
fn floors_json_is_parseable_array() {
    let out = cargo_evidence()
        .current_dir(workspace_root())
        .args(["evidence", "floors", "--json"])
        .output()
        .expect("spawn");
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    let v: Value = serde_json::from_str(&stdout).expect("valid JSON");
    assert!(v.is_array(), "top-level must be array");
    for row in v.as_array().unwrap() {
        assert!(row.get("name").is_some());
        assert!(row.get("kind").is_some());
        assert!(row.get("current").is_some());
        assert!(row.get("floor").is_some());
        assert!(row.get("status").is_some());
    }
}
