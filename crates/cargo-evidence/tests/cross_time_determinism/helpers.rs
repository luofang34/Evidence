//! Shared helpers for `cross_time_determinism.rs`. Split out of
//! the parent file to stay under the 500-line workspace size
//! limit. Everything here is fixture construction + subprocess
//! plumbing; the actual `#[test]` cases live in the parent.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    dead_code,
    reason = "test-only helpers; parent uses a subset per-case"
)]

use std::path::PathBuf;
use std::process::Command;

use tempfile::TempDir;

pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

pub fn lint_script() -> PathBuf {
    workspace_root()
        .join("scripts")
        .join("deterministic-baseline-override-lint.sh")
}

/// The one legitimate reason the tests can't run: the script
/// itself isn't reachable from the test binary. Nix
/// `buildRustPackage` copies only the crate's src tree — if a
/// future Nix config forgets to expose `scripts/`, tests skip
/// rather than panic on a `NotFound` spawn.
///
/// `jq` availability is NOT part of this probe. The project
/// philosophy is "mechanical guardrails > graceful degradation";
/// silently skipping nine tests in the Nix gate defeats the
/// gate's whole purpose (Nix is specifically meant to validate
/// the reproducibility path). `flake.nix` puts `jq` in the
/// sandbox's `nativeBuildInputs`, so a missing `jq` indicates a
/// real misconfiguration that should fail loud with "jq is
/// required but not found on PATH" from the script itself.
///
/// Returns `true` iff the script is reachable.
pub fn script_available() -> bool {
    lint_script().is_file()
}

/// Minimal `deterministic-manifest.json` stub carrying the six
/// toolchain-sensitive fields the lint projects. Everything else
/// (schema_version, profile, git_*) is irrelevant to the gate and
/// omitted so the fixtures stay tiny and obvious.
pub fn manifest_json(
    rustc: &str,
    cargo: &str,
    llvm: Option<&str>,
    cargo_lock_hash: &str,
    rust_toolchain_toml: &str,
    rustflags: Option<&str>,
) -> String {
    let llvm_field = match llvm {
        Some(v) => format!(r#""llvm_version":"{}""#, v),
        None => r#""llvm_version":null"#.to_string(),
    };
    let rustflags_field = match rustflags {
        Some(v) => format!(r#""rustflags":"{}""#, v),
        None => r#""rustflags":null"#.to_string(),
    };
    format!(
        r#"{{"rustc":"{}","cargo":"{}",{},"cargo_lock_hash":"{}","rust_toolchain_toml":{},{}}}"#,
        rustc,
        cargo,
        llvm_field,
        cargo_lock_hash,
        serde_json::to_string(rust_toolchain_toml).expect("serialize toolchain string"),
        rustflags_field,
    )
}

pub fn write_manifest(dir: &TempDir, name: &str, contents: &str) -> PathBuf {
    let path = dir.path().join(name);
    std::fs::write(&path, contents).expect("write fixture");
    path
}

pub fn default_prior_and_current(dir: &TempDir) -> (PathBuf, PathBuf) {
    // Same six fields on both sides = identity; tests that need
    // drift override one of them.
    let prior = manifest_json(
        "rustc 1.95.0 (abc)",
        "cargo 1.95.0 (abc)",
        Some("20.0.0"),
        "deadbeef".to_string().as_str(),
        "[toolchain]\nchannel = \"1.95\"\n",
        Some("-D warnings"),
    );
    let current = prior.clone();
    (
        write_manifest(dir, "prior.json", &prior),
        write_manifest(dir, "current.json", &current),
    )
}

/// Spawn the lint via `bash <script>` (not by path) so the test
/// is portable across Linux + macOS regardless of which `env`
/// discovery path the shebang takes.
pub fn run_lint(
    prior: &PathBuf,
    current: &PathBuf,
    pr_body: Option<&str>,
    commit_msg: Option<&str>,
) -> (i32, String) {
    let mut cmd = Command::new("bash");
    cmd.arg(lint_script()).arg(prior).arg(current);
    if let Some(body) = pr_body {
        cmd.env("PR_BODY", body);
    } else {
        cmd.env_remove("PR_BODY");
    }
    if let Some(msg) = commit_msg {
        cmd.env("COMMIT_MESSAGE", msg);
    } else {
        cmd.env_remove("COMMIT_MESSAGE");
    }
    let out = cmd.output().expect("spawn lint script");
    let code = out.status.code().unwrap_or(-1);
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    (code, stderr)
}
