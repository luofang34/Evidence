//! Behavioral tests for `scripts/cross-time-fetch-baseline.sh`.
//!
//! The cross-time gate's fail-closed guarantee is only real if the
//! baseline-fetch script actually exits non-zero — with the right
//! diagnostic — when the base-SHA baseline is missing / unavailable /
//! malformed. A workflow-text grep can't prove that; it would still
//! pass if `exit 1` were deleted. So these run the real script against a
//! fake `gh` and assert both the exit status AND the specific `::error::`
//! line, so a harness failure (e.g. the fake failing to launch) can't
//! masquerade as a passing negative case:
//!
//! - no successful base run          → non-zero + "No successful ci.yml run"
//! - artifact download failure        → non-zero + "unavailable"
//! - artifact missing the manifest    → non-zero + "missing deterministic-manifest.json"
//! - manifest is malformed JSON       → non-zero + "malformed"
//! - a well-formed baseline           → zero, and `prior_missing=0`
//!
//! The fake `gh` is invoked as `bash <file>` (via the script's `GH`
//! override), not through a shebang — a pure Nix build sandbox lacks
//! `/usr/bin/env`, which exit-126s a `#!/usr/bin/env` fake. Unix-only
//! (bash); the `#[test]` lines are still counted by the `test_count`
//! floor on every OS (a source-text count).
#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use std::fs;
use std::path::PathBuf;
use std::process::Command;

/// A fake `gh` driven by `$FAKE_GH_SCENARIO`. Invoked as `bash <this>`,
/// so it needs no shebang or executable bit. Answers `gh api …` (prints
/// a run id, or nothing for `no_run`) and `gh run download … --dir <d>`
/// (fails, or writes an empty / malformed / valid manifest).
const FAKE_GH: &str = r#"
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
    malformed_manifest)
      mkdir -p "$dest"
      printf '%s' 'not json {{{' > "$dest/deterministic-manifest.json"
      exit 0 ;;
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

struct Outcome {
    code: i32,
    prior: String,
    output: String,
}

/// Run the fetch script under a fake `gh` for `scenario`.
fn run_scenario(scenario: &str) -> Outcome {
    let tmp = tempfile::tempdir().expect("tempdir");
    let fake = tmp.path().join("fake-gh");
    fs::write(&fake, FAKE_GH).expect("write fake gh");

    let dest = tmp.path().join("prior-main");
    let out_file = tmp.path().join("gh_output");
    fs::write(&out_file, "").expect("touch output");

    let script = workspace_root()
        .join("scripts")
        .join("cross-time-fetch-baseline.sh");

    let out = Command::new("bash")
        .arg(&script)
        .arg(&dest)
        .env("GH", format!("bash {}", fake.display()))
        .env("FAKE_GH_SCENARIO", scenario)
        .env("BASE_SHA", "deadbeefcafefeed")
        .env("GITHUB_REPOSITORY", "owner/repo")
        .env("GITHUB_OUTPUT", &out_file)
        .output()
        .expect("run baseline script");

    let mut output = String::from_utf8_lossy(&out.stdout).into_owned();
    output.push_str(&String::from_utf8_lossy(&out.stderr));
    Outcome {
        code: out.status.code().unwrap_or(-1),
        prior: fs::read_to_string(&out_file).unwrap_or_default(),
        output,
    }
}

/// Assert a negative scenario failed closed with the expected
/// diagnostic — never merely "some non-zero", so a broken harness can't
/// pass. Includes the captured output on failure.
fn assert_fails_closed(scenario: &str, needle: &str) {
    let o = run_scenario(scenario);
    assert_ne!(
        o.code, 0,
        "scenario `{scenario}` must fail closed (non-zero exit).\n--- output ---\n{}",
        o.output
    );
    assert!(
        o.output.contains("::error::"),
        "scenario `{scenario}` must emit a `::error::` diagnostic.\n--- output ---\n{}",
        o.output
    );
    assert!(
        o.output.contains(needle),
        "scenario `{scenario}` diagnostic must mention `{needle}`.\n--- output ---\n{}",
        o.output
    );
    assert!(
        !o.prior.contains("prior_missing=0"),
        "scenario `{scenario}` must not signal a usable baseline.\n--- output ---\n{}",
        o.output
    );
}

#[test]
fn no_successful_base_run_fails_closed() {
    assert_fails_closed("no_run", "No successful ci.yml run for base SHA");
}

#[test]
fn artifact_download_failure_fails_closed() {
    assert_fails_closed("download_fail", "is unavailable (expired or missing)");
}

#[test]
fn missing_manifest_fails_closed() {
    assert_fails_closed("missing_manifest", "is missing deterministic-manifest.json");
}

#[test]
fn malformed_manifest_fails_closed() {
    assert_fails_closed(
        "malformed_manifest",
        "has a malformed deterministic-manifest.json",
    );
}

#[test]
fn valid_baseline_succeeds_with_prior_missing_zero() {
    let o = run_scenario("valid");
    assert_eq!(
        o.code, 0,
        "a usable baseline must exit zero.\n--- output ---\n{}",
        o.output
    );
    assert!(
        o.prior.contains("prior_missing=0"),
        "a usable baseline must set prior_missing=0, got {:?}.\n--- output ---\n{}",
        o.prior,
        o.output
    );
}
