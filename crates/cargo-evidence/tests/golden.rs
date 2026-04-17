//! Golden-bundle regression test.
//!
//! Locks the on-disk bundle format. `tests/fixtures/golden-dev-bundle/` is a
//! committed, canonical evidence bundle; if any change in the engine shifts
//! how bundles are written, verified, or diffed, one of the two assertions
//! below fails and the same PR must regenerate the fixture. Forces the
//! format migration to be explicit rather than silent.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use assert_cmd::Command;
use evidence::verify::verify_bundle;
use std::path::PathBuf;

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/golden-dev-bundle")
}

fn cargo_evidence() -> Command {
    #[allow(deprecated)]
    Command::cargo_bin("cargo-evidence").unwrap()
}

#[test]
fn golden_bundle_verify_returns_pass() {
    let fixture = fixture_path();
    assert!(
        fixture.is_dir(),
        "fixture missing at {:?} — did you delete tests/fixtures/golden-dev-bundle?",
        fixture
    );

    let result = verify_bundle(&fixture).expect("verify_bundle must not error on fixture");
    assert!(
        result.is_pass(),
        "golden bundle failed verify_bundle:\n  result: {:?}\n  summary: {}\n\
         If you broke the bundle format or hashing, regenerate the fixture in THIS PR:\n  \
         `cargo run --release -p cargo-evidence -- evidence generate \\\n    \
         --skip-tests --profile dev --out-dir <tmpdir>` and copy the produced dir over \
         tests/fixtures/golden-dev-bundle.",
        result,
        result.summary()
    );
}

#[test]
fn golden_bundle_diffs_empty_against_itself() {
    let fixture = fixture_path();
    assert!(fixture.is_dir(), "fixture missing at {:?}", fixture);

    let output = cargo_evidence()
        .arg("evidence")
        .arg("diff")
        .arg(&fixture)
        .arg(&fixture)
        .arg("--json")
        .output()
        .expect("failed to run cargo-evidence diff");

    assert!(
        output.status.success(),
        "cargo-evidence diff failed: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("diff json stdout must be UTF-8");
    let diff: serde_json::Value =
        serde_json::from_str(&stdout).expect("diff stdout must be valid JSON");

    for section in ["inputs_diff", "outputs_diff"] {
        for key in ["added", "removed", "changed"] {
            let arr = diff[section][key]
                .as_array()
                .unwrap_or_else(|| panic!("{}.{} must be an array", section, key));
            assert!(
                arr.is_empty(),
                "self-diff must be empty, but {}.{} = {:?}",
                section,
                key,
                arr
            );
        }
    }
    for field in ["profile", "git_sha", "git_branch", "git_dirty"] {
        assert!(
            diff["metadata_diff"][field].is_null(),
            "self-diff must report no metadata change, but metadata_diff.{} = {:?}",
            field,
            diff["metadata_diff"][field]
        );
    }
    let env_diff = diff["env_diff"]
        .as_array()
        .expect("env_diff must be an array");
    assert!(
        env_diff.is_empty(),
        "self-diff env_diff must be empty, got {:?}",
        env_diff
    );
}
