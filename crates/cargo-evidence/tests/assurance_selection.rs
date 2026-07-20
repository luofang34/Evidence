//! Integration tests for the explicit-assurance-selection gate
//! (HLR-089 / LLR-109).
//!
//! Cert/record named-claim evaluation fails closed when the
//! boundary carries no explicit selection; development mode
//! constructs the unclassified selection and names it — never a
//! silent DAL-D. Lives as its own integration-test file so the
//! generate orchestrator stays under the workspace 500-line limit.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use std::fs;
use std::path::Path;

use assert_cmd::Command;
use tempfile::TempDir;

fn cargo_evidence() -> Command {
    #[allow(deprecated)]
    Command::cargo_bin("cargo-evidence").unwrap()
}

/// Minimal boundary fixture: schema + in-scope list + policy, but
/// deliberately NO `[dal]` section.
fn write_boundary_without_dal(root: &Path, in_scope: &[&str]) {
    fs::create_dir_all(root.join("cert")).unwrap();
    let in_scope_list = in_scope
        .iter()
        .map(|s| format!("\"{s}\""))
        .collect::<Vec<_>>()
        .join(", ");
    fs::write(
        root.join("cert").join("boundary.toml"),
        format!(
            r#"[schema]
version = "{ver}"

[scope]
in_scope = [{in_scope_list}]

[policy]
no_out_of_scope_deps = false
"#,
            ver = evidence_core::schema_versions::BOUNDARY
        ),
    )
    .unwrap();
}

fn git(root: &Path, args: &[&str]) {
    let status = std::process::Command::new("git")
        .args(args)
        .current_dir(root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .status()
        .expect("spawn git");
    assert!(status.success(), "git {args:?} failed");
}

/// Scaffold a real (tiny) cargo + git workspace so the dev
/// generate reaches the compliance phase end to end: cargo
/// metadata must resolve the in-scope package and `git ls-files`
/// must enumerate its sources. The package lives in a
/// subdirectory — a root-manifest package resolves to an empty
/// relative pathspec that `git ls-files` rejects.
fn scaffold_workspace(root: &Path) {
    let pkg = root.join("fixture_lib");
    fs::create_dir_all(pkg.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        r#"[workspace]
members = ["fixture_lib"]
resolver = "2"
"#,
    )
    .unwrap();
    fs::write(
        pkg.join("Cargo.toml"),
        r#"[package]
name = "fixture_lib"
version = "0.1.0"
edition = "2021"
"#,
    )
    .unwrap();
    fs::write(pkg.join("src").join("lib.rs"), "pub fn f() -> u8 { 1 }\n").unwrap();
    git(root, &["init", "-q"]);
    git(root, &["add", "-A"]);
    git(
        root,
        &[
            "-c",
            "user.email=test@example.com",
            "-c",
            "user.name=test",
            "commit",
            "-qm",
            "init",
        ],
    );
}

/// **Fail closed, missing boundary.** `--profile cert` in a
/// workspace with no `cert/boundary.toml` at all must refuse the
/// run with `POLICY_ASSURANCE_SELECTION_MISSING` and a non-zero
/// exit — before any tolerant default loading can invent a level.
#[test]
fn generate_cert_without_boundary_fails_closed() {
    let tmp = TempDir::new().unwrap();
    let out = TempDir::new().unwrap();
    let result = cargo_evidence()
        .arg("evidence")
        .arg("generate")
        .arg("--out-dir")
        .arg(out.path())
        .arg("--profile")
        .arg("cert")
        .current_dir(tmp.path())
        .output()
        .unwrap();
    assert!(!result.status.success(), "cert must fail closed");
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr.contains("POLICY_ASSURANCE_SELECTION_MISSING"),
        "stderr must name the typed code; got:\n{stderr}"
    );
}

/// **Fail closed, absent [dal].** A loadable boundary with
/// in-scope crates but no `[dal]` section is still a missing
/// selection: cert evaluation must not silently assume one.
#[test]
fn generate_cert_without_dal_section_fails_closed() {
    let tmp = TempDir::new().unwrap();
    write_boundary_without_dal(tmp.path(), &["fixture_lib"]);
    let out = TempDir::new().unwrap();
    let result = cargo_evidence()
        .arg("evidence")
        .arg("generate")
        .arg("--out-dir")
        .arg(out.path())
        .arg("--profile")
        .arg("cert")
        .current_dir(tmp.path())
        .output()
        .unwrap();
    assert!(!result.status.success(), "cert must fail closed");
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr.contains("POLICY_ASSURANCE_SELECTION_MISSING"),
        "stderr must name the typed code; got:\n{stderr}"
    );
}

/// **Development mode names unclassified.** The same missing
/// selection on the dev profile must NOT fail — but the
/// compliance report it writes names the level `unclassified`,
/// never a DAL letter, so a missing configuration cannot
/// masquerade as a DAL-D claim.
#[test]
fn generate_dev_without_dal_section_names_unclassified_in_compliance_report() {
    let tmp = TempDir::new().unwrap();
    scaffold_workspace(tmp.path());
    write_boundary_without_dal(tmp.path(), &["fixture_lib"]);

    let out = TempDir::new().unwrap();
    cargo_evidence()
        .arg("evidence")
        .arg("generate")
        .arg("--skip-tests")
        .arg("--out-dir")
        .arg(out.path())
        .arg("--profile")
        .arg("dev")
        .current_dir(tmp.path())
        .assert()
        .success();

    let bundle = fs::read_dir(out.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| {
            p.is_dir()
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .starts_with("dev-")
        })
        .expect("bundle directory under out_dir");
    let report_path = bundle.join("compliance").join("fixture_lib.json");
    let report: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(&report_path).expect("compliance report for fixture_lib"),
    )
    .expect("compliance report is valid JSON");
    assert_eq!(report["assurance_level"], "unclassified");
    assert_eq!(report["dal"], "unclassified");
    assert_eq!(report["standard"], "DO-178C");
    assert_eq!(report["standard_edition"], "C");
    assert_eq!(report["standards_pack"]["id"], "do-178c-ac20-115d");
    // The bundle index must agree with the report (verify's
    // dal_map consistency check reads both).
    let index: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(bundle.join("index.json")).expect("index.json"))
            .expect("index.json is valid JSON");
    assert_eq!(index["dal_map"]["fixture_lib"], "unclassified");
}
