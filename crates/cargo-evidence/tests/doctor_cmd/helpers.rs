//! Shared helpers for `doctor_cmd.rs`. Split out of the parent file
//! to stay under the 500-line workspace file-size limit. Fixture
//! construction + subprocess plumbing; `#[test]` cases live in the
//! parent.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    dead_code,
    reason = "test-only helpers; parent uses a subset per-case"
)]

use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use serde_json::Value;
use tempfile::TempDir;

pub fn cargo_evidence() -> Command {
    #[allow(deprecated)]
    Command::cargo_bin("cargo-evidence").unwrap()
}

pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

/// Build a synthetic "rigorous" fixture — has everything doctor
/// checks for. Uses symlinks to the real `cert/trace/` on Unix so
/// the trace validator runs against schema-valid entries; Windows
/// falls back to copying the four toml files.
pub fn setup_rigorous_fixture() -> TempDir {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path();

    let real = workspace_root();
    fs::create_dir_all(root.join("cert")).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(
        real.join("cert").join("trace"),
        root.join("cert").join("trace"),
    )
    .unwrap();
    #[cfg(not(unix))]
    {
        let fake_trace = root.join("cert").join("trace");
        fs::create_dir_all(&fake_trace).unwrap();
        for name in ["sys.toml", "hlr.toml", "llr.toml", "tests.toml"] {
            fs::copy(
                real.join("cert").join("trace").join(name),
                fake_trace.join(name),
            )
            .unwrap();
        }
    }

    fs::create_dir_all(root.join("cert")).unwrap();
    fs::write(
        root.join("cert").join("floors.toml"),
        "schema_version = 1\n\n[floors]\n",
    )
    .unwrap();
    fs::write(
        root.join("cert").join("boundary.toml"),
        "[schema]\nversion = \"0.0.1\"\n\n\
         [scope]\nin_scope = [\"evidence\"]\ntrace_roots = [\"cert/trace\"]\n\n\
         [policy]\nno_out_of_scope_deps = false\n\
         forbid_build_rs = false\n\
         forbid_proc_macros = false\n",
    )
    .unwrap();

    let wf = root.join(".github").join("workflows");
    fs::create_dir_all(&wf).unwrap();
    fs::write(
        wf.join("ci.yml"),
        "name: CI\non: push\njobs:\n  check:\n    runs-on: ubuntu-latest\n    \
         steps:\n      - run: cargo evidence check\n",
    )
    .unwrap();
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
pub fn run_doctor(workspace: &Path) -> (i32, Vec<Value>) {
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
