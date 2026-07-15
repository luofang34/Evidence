//! Behavioral tests for `scripts/cross-time-fetch-baseline.sh`.
//!
//! The cross-time gate's fail-closed guarantee is only real if the
//! baseline-fetch script actually exits non-zero when the base-SHA
//! baseline is missing / unavailable / malformed. A workflow-text grep
//! can't prove that — it would still pass if `exit 1` were deleted. So
//! these run the real script against a fake `gh` and assert exit codes:
//!
//! - no successful base run        → non-zero (fail closed)
//! - artifact download failure      → non-zero
//! - artifact missing the manifest  → non-zero
//! - a usable baseline              → zero, and `prior_missing=0`
//!
//! Unix-only: the script is bash and the harness sets an executable bit.
//! The `#[test]` lines are still counted by the `test_count` floor on
//! every OS (a source-text count), so the ratchet stays consistent.
#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;

/// A fake `gh` whose responses are driven by `$FAKE_GH_SCENARIO`. It
/// answers the two subcommands the script uses: `gh api …` (prints a run
/// id, or nothing for `no_run`) and `gh run download … --dir <dest>`
/// (fails, creates an empty dir, or writes a manifest).
const FAKE_GH: &str = r#"#!/usr/bin/env bash
sub="$1"
if [ "$sub" = "api" ]; then
  case "${FAKE_GH_SCENARIO}" in
    no_run) exit 0 ;;
    *) echo "12345" ;;
  esac
  exit 0
fi
if [ "$sub" = "run" ]; then
  dest=""
  shift
  while [ $# -gt 0 ]; do
    if [ "$1" = "--dir" ]; then dest="$2"; fi
    shift
  done
  case "${FAKE_GH_SCENARIO}" in
    download_fail) exit 1 ;;
    missing_manifest) mkdir -p "$dest"; exit 0 ;;
    valid)
      mkdir -p "$dest"
      printf '%s' '{"rustc":"rustc 1.95.0","cargo":"cargo 1.95.0","llvm_version":"22","cargo_lock_hash":"abc123","rustflags":"-D warnings"}' > "$dest/deterministic-manifest.json"
      exit 0 ;;
    *) exit 1 ;;
  esac
fi
exit 0
"#;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

/// Run the fetch script under a fake `gh` for `scenario`. Returns
/// `(exit_code, github_output_contents)`.
fn run_scenario(scenario: &str) -> (i32, String) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let bin = tmp.path().join("bin");
    fs::create_dir_all(&bin).expect("mkdir bin");
    let gh = bin.join("gh");
    fs::write(&gh, FAKE_GH).expect("write fake gh");
    fs::set_permissions(&gh, fs::Permissions::from_mode(0o755)).expect("chmod");

    let dest = tmp.path().join("prior-main");
    let out_file = tmp.path().join("gh_output");
    fs::write(&out_file, "").expect("touch output");

    let script = workspace_root()
        .join("scripts")
        .join("cross-time-fetch-baseline.sh");
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );

    let output = Command::new("bash")
        .arg(&script)
        .arg(&dest)
        .env("PATH", path)
        .env("FAKE_GH_SCENARIO", scenario)
        .env("BASE_SHA", "deadbeefcafefeed")
        .env("GITHUB_REPOSITORY", "owner/repo")
        .env("GITHUB_OUTPUT", &out_file)
        .output()
        .expect("run baseline script");

    let code = output.status.code().unwrap_or(-1);
    let prior = fs::read_to_string(&out_file).unwrap_or_default();
    (code, prior)
}

#[test]
fn no_successful_base_run_fails_closed() {
    let (code, prior) = run_scenario("no_run");
    assert_ne!(code, 0, "no successful base run must fail closed");
    assert!(
        !prior.contains("prior_missing=0"),
        "must not signal a usable baseline"
    );
}

#[test]
fn artifact_download_failure_fails_closed() {
    let (code, _) = run_scenario("download_fail");
    assert_ne!(code, 0, "an artifact download failure must fail closed");
}

#[test]
fn missing_manifest_fails_closed() {
    let (code, _) = run_scenario("missing_manifest");
    assert_ne!(
        code, 0,
        "an artifact without deterministic-manifest.json must fail closed"
    );
}

#[test]
fn valid_baseline_succeeds_with_prior_missing_zero() {
    let (code, prior) = run_scenario("valid");
    assert_eq!(code, 0, "a usable baseline must exit zero");
    assert!(
        prior.contains("prior_missing=0"),
        "a usable baseline must set prior_missing=0, got: {prior:?}"
    );
}
