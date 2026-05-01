//! Integration tests for the DAL-A MC/DC qualification gate
//! (HLR-066 / LLR-073).
//!
//! Lives as its own integration-test file (rather than a sibling
//! function in `cli.rs`) so the orchestrator stays under the
//! workspace 500-line limit.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use std::fs;

use assert_cmd::Command;
use tempfile::TempDir;

fn cargo_evidence() -> Command {
    #[allow(deprecated)]
    Command::cargo_bin("cargo-evidence").unwrap()
}

fn write_dal_a_boundary_toml(path: &std::path::Path) {
    fs::create_dir_all(path.join("cert")).unwrap();
    fs::write(
        path.join("cert/boundary.toml"),
        format!(
            r#"
[schema]
version = "{ver}"

[scope]
in_scope = ["flight_core"]

[policy]
no_out_of_scope_deps = false
forbid_build_rs = false
forbid_proc_macros = false

[dal]
default_dal = "A"
"#,
            ver = evidence_core::schema_versions::BOUNDARY
        ),
    )
    .unwrap();
}

/// TEST-080 integration arm. Set up a fixture workspace with an
/// in-scope crate at DAL-A and no `[dal.auxiliary_mcdc_tool]`, then
/// run `cargo evidence generate --profile dev`. The DAL-A MC/DC
/// gate must emit a `warning:` line on stderr (LLR-073's dev-side
/// soft path) and not abort on that gate alone — downstream phases
/// may still fail because the tempdir isn't a real workspace, but
/// the failure must NOT come from the DAL-A gate's `error:` envelope.
#[test]
fn test_generate_dev_profile_warns_on_dal_a_without_mcdc_tool_but_continues() {
    let tmp = TempDir::new().unwrap();
    write_dal_a_boundary_toml(tmp.path());

    let out = TempDir::new().unwrap();
    let result = cargo_evidence()
        .arg("evidence")
        .arg("generate")
        .arg("--out-dir")
        .arg(out.path())
        .arg("--profile")
        .arg("dev")
        .current_dir(tmp.path())
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&result.stderr);
    // Dev path must emit the gate's prose with a `warning:` prefix
    // so an iterating user sees the issue but isn't blocked by the
    // gate itself. (Later phases — cargo metadata against a tempdir
    // that isn't a real workspace, missing trace roots — may still
    // fail downstream; the warn-and-continue contract is purely
    // about THIS gate's behavior.)
    assert!(
        stderr.contains("warning: DAL-A qualification gap"),
        "expected dev-profile DAL-A warning prefixed with `warning:`, got stderr:\n{}",
        stderr
    );
    // The fail-loud envelope at cert/record uses `error:` (via the
    // shared `fail` helper). Pin that the dev path took the warn
    // branch by absence of `error: DAL-A qualification gap`.
    assert!(
        !stderr.contains("error: DAL-A qualification gap"),
        "dev profile must not emit the cert-style `error:` envelope; stderr:\n{}",
        stderr
    );
}
