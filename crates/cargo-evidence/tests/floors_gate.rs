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
