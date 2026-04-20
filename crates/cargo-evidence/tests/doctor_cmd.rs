//! Integration tests for `cargo evidence doctor` (TEST-048).
//!
//! Covers the four selectors TEST-048 pins:
//! - `rigorous_fixture_passes` — synthetic fixture with trace +
//!   floors + boundary + ci.yml + README override section passes
//!   with `DOCTOR_OK` + 6 `DOCTOR_CHECK_PASSED`.
//! - `sloppy_fixture_fails_with_named_codes` — fixture missing
//!   trace + floors + boundary fires with specific fail codes and
//!   the `DOCTOR_FAIL` terminal.
//! - `cert_generate_blocks_on_doctor_fail` — `generate --profile
//!   cert` on a sloppy fixture aborts before bundle assembly,
//!   surfacing the doctor codes in the error message.
//! - `current_workspace_passes_doctor` — our own repo passes all
//!   6 checks (load-bearing regression: if the tool's own rigor
//!   slips, this fires).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use serde_json::Value;
use tempfile::TempDir;

fn cargo_evidence() -> Command {
    #[allow(deprecated)]
    Command::cargo_bin("cargo-evidence").unwrap()
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

/// Build a synthetic "rigorous" fixture — has everything doctor
/// checks for. Uses symlinks to the real `tool/trace/` on Unix so
/// the trace validator runs against schema-valid entries; Windows
/// falls back to copying the four toml files.
fn setup_rigorous_fixture() -> TempDir {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path();

    // tool/trace — the real one (symlink on Unix, copy on Windows).
    let real = workspace_root();
    fs::create_dir_all(root.join("tool")).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(
        real.join("tool").join("trace"),
        root.join("tool").join("trace"),
    )
    .unwrap();
    #[cfg(not(unix))]
    {
        let fake_trace = root.join("tool").join("trace");
        fs::create_dir_all(&fake_trace).unwrap();
        for name in ["sys.toml", "hlr.toml", "llr.toml", "tests.toml"] {
            fs::copy(
                real.join("tool").join("trace").join(name),
                fake_trace.join(name),
            )
            .unwrap();
        }
    }

    // cert/floors.toml — minimal schema_version + [floors] so
    // loading succeeds. No dimensions = nothing breaches.
    fs::create_dir_all(root.join("cert")).unwrap();
    fs::write(
        root.join("cert").join("floors.toml"),
        "schema_version = 1\n\n[floors]\n",
    )
    .unwrap();

    // cert/boundary.toml — minimal shape: schema + scope + policy.
    fs::write(
        root.join("cert").join("boundary.toml"),
        "[schema]\nversion = \"0.0.1\"\n\n\
         [scope]\nin_scope = [\"evidence\"]\ntrace_roots = [\"tool/trace\"]\n\n\
         [policy]\nno_out_of_scope_deps = false\n\
         forbid_build_rs = false\n\
         forbid_proc_macros = false\n",
    )
    .unwrap();

    // .github/workflows/ci.yml mentioning cargo evidence.
    let wf = root.join(".github").join("workflows");
    fs::create_dir_all(&wf).unwrap();
    fs::write(
        wf.join("ci.yml"),
        "name: CI\non: push\njobs:\n  check:\n    runs-on: ubuntu-latest\n    \
         steps:\n      - run: cargo evidence check\n",
    )
    .unwrap();

    // README.md mentioning Override-Deterministic-Baseline:
    fs::write(
        root.join("README.md"),
        "# Fixture repo\n\nFor reproducibility-affecting changes, add \
         `Override-Deterministic-Baseline: <reason>` to the PR body.\n",
    )
    .unwrap();

    tmp
}

/// Run `cargo evidence doctor --format=jsonl` against `workspace`,
/// returning (exit_code, parsed_diagnostics).
fn run_doctor(workspace: &Path) -> (i32, Vec<Value>) {
    let out = cargo_evidence()
        .args(["evidence", "doctor", "--format=jsonl"])
        .current_dir(workspace)
        .output()
        .expect("spawn cargo-evidence");
    let exit = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let diags: Vec<Value> = stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("each line must be a JSON object"))
        .collect();
    (exit, diags)
}

#[test]
fn rigorous_fixture_passes() {
    let tmp = setup_rigorous_fixture();
    let (exit, diags) = run_doctor(tmp.path());
    assert_eq!(exit, 0, "rigorous fixture should exit 0; diags={:?}", diags);
    let codes: Vec<&str> = diags.iter().map(|d| d["code"].as_str().unwrap()).collect();
    // Exactly 6 DOCTOR_CHECK_PASSED + 1 DOCTOR_OK terminal.
    let passed_count = codes
        .iter()
        .filter(|c| **c == "DOCTOR_CHECK_PASSED")
        .count();
    assert_eq!(
        passed_count, 6,
        "expected 6 DOCTOR_CHECK_PASSED diagnostics; got codes={:?}",
        codes
    );
    assert_eq!(
        codes.last().copied(),
        Some("DOCTOR_OK"),
        "stream must terminate with DOCTOR_OK; got codes={:?}",
        codes
    );
}

#[test]
fn sloppy_fixture_fails_with_named_codes() {
    // Empty tempdir — no tool/trace, no cert/, no .github/, no README.
    let tmp = TempDir::new().expect("tempdir");
    let (exit, diags) = run_doctor(tmp.path());
    assert_eq!(exit, 2, "sloppy fixture should exit 2; diags={:?}", diags);
    let codes: Vec<&str> = diags.iter().map(|d| d["code"].as_str().unwrap()).collect();
    assert!(
        codes.contains(&"DOCTOR_TRACE_INVALID"),
        "expected DOCTOR_TRACE_INVALID in codes={:?}",
        codes
    );
    assert!(
        codes.contains(&"DOCTOR_FLOORS_MISSING"),
        "expected DOCTOR_FLOORS_MISSING in codes={:?}",
        codes
    );
    assert!(
        codes.contains(&"DOCTOR_BOUNDARY_MISSING"),
        "expected DOCTOR_BOUNDARY_MISSING in codes={:?}",
        codes
    );
    assert_eq!(
        codes.last().copied(),
        Some("DOCTOR_FAIL"),
        "stream must terminate with DOCTOR_FAIL; got codes={:?}",
        codes
    );
}

#[test]
fn cert_generate_blocks_on_doctor_fail() {
    // Sloppy fixture: no trace, no floors, no boundary.
    let tmp = TempDir::new().expect("tempdir");
    // A minimal Cargo.toml so `generate`'s preflight + profile resolution
    // doesn't fail on something upstream of the doctor precheck.
    fs::write(
        tmp.path().join("Cargo.toml"),
        "[workspace]\nmembers = []\nresolver = \"2\"\n",
    )
    .unwrap();

    let out_dir = TempDir::new().expect("tempdir").keep();
    let out = cargo_evidence()
        .args(["evidence", "generate", "--profile", "cert", "--out-dir"])
        .arg(&out_dir)
        .current_dir(tmp.path())
        .output()
        .expect("spawn");
    let exit = out.status.code().unwrap_or(-1);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_ne!(
        exit, 0,
        "cert-profile generate on sloppy fixture must abort; exit={}\nstderr:\n{}\nstdout:\n{}",
        exit, stderr, stdout
    );
    // The precheck error message enumerates the triggered
    // DOCTOR_* codes via the anyhow chain.
    let combined = format!("{}{}", stderr, stdout);
    assert!(
        combined.contains("doctor precheck failed")
            || combined.contains("DOCTOR_TRACE_INVALID")
            || combined.contains("DOCTOR_FLOORS_MISSING")
            || combined.contains("DOCTOR_BOUNDARY_MISSING"),
        "stderr/stdout must surface the doctor codes; \nstderr:\n{}\nstdout:\n{}",
        stderr,
        stdout,
    );
}

#[test]
fn current_workspace_passes_doctor() {
    // Load-bearing regression: if the tool's own rigor slips below
    // its published checklist (trace breaks, floors drift, README
    // drops the override section), this test fires.
    let workspace = workspace_root();
    let (exit, diags) = run_doctor(&workspace);
    assert_eq!(
        exit, 0,
        "cargo-evidence's own workspace must pass its own doctor; \
         diags={:?}",
        diags
    );
    let codes: Vec<&str> = diags.iter().map(|d| d["code"].as_str().unwrap()).collect();
    assert_eq!(
        codes.last().copied(),
        Some("DOCTOR_OK"),
        "self-dogfood must terminate with DOCTOR_OK; got codes={:?}",
        codes
    );
}
