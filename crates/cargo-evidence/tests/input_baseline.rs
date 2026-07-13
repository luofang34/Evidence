//! Integration guard for the source-input baseline (#138 / SYS-032).
//!
//! Generates a real bundle from this workspace and asserts that
//! `inputs_hashes.json` (a) is non-empty and (b) agrees exactly with an
//! independent `git ls-files` enumeration over the in-scope crate
//! directories plus the workspace-control pathspecs. This is the
//! end-to-end check that would have caught the original defect, where
//! bare package names were handed to `git ls-files` as pathspecs and
//! matched nothing.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use assert_cmd::Command;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root is two levels above the crate manifest dir")
        .to_path_buf()
}

/// Independent enumeration: everything `git ls-files` reports under the
/// three in-scope crate directories plus the workspace-control
/// pathspecs. Mirrors what the generator must capture, computed a
/// different way (one git call, no cargo-metadata resolution).
fn independent_enumeration(root: &Path) -> BTreeSet<String> {
    let out = StdCommand::new("git")
        .current_dir(root)
        .args([
            "ls-files",
            "-z",
            "--",
            "crates/evidence-core",
            "crates/cargo-evidence",
            "crates/evidence-mcp",
            "Cargo.toml",
            "Cargo.lock",
            "rust-toolchain.toml",
            "rust-toolchain",
            "cert",
        ])
        .output()
        .expect("git ls-files");
    assert!(out.status.success(), "git ls-files must succeed");
    out.stdout
        .split(|b| *b == 0)
        .filter(|s| !s.is_empty())
        .map(|s| String::from_utf8(s.to_vec()).expect("utf-8 path"))
        .collect()
}

#[test]
fn captured_baseline_agrees_with_independent_enumeration() {
    let root = workspace_root();
    let tmp = tempfile::tempdir().expect("tempdir");

    Command::cargo_bin("cargo-evidence")
        .unwrap()
        .current_dir(&root)
        .args(["evidence", "generate", "--skip-tests", "--profile", "dev"])
        .arg("--out-dir")
        .arg(tmp.path())
        .assert()
        .success();

    let bundle = walkdir::WalkDir::new(tmp.path())
        .follow_links(false)
        .min_depth(1)
        .max_depth(1)
        .into_iter()
        .filter_map(Result::ok)
        .map(walkdir::DirEntry::into_path)
        .find(|p| {
            p.is_dir()
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("dev-"))
        })
        .expect("bundle directory under out_dir");

    let inputs_json = std::fs::read_to_string(bundle.join("inputs_hashes.json")).unwrap();
    let inputs: std::collections::BTreeMap<String, String> =
        serde_json::from_str(&inputs_json).unwrap();

    assert!(
        !inputs.is_empty(),
        "inputs_hashes.json must not be empty for a workspace with in-scope crates"
    );

    let captured: BTreeSet<String> = inputs.keys().cloned().collect();
    let expected = independent_enumeration(&root);

    assert_eq!(
        captured,
        expected,
        "captured baseline diverged from independent enumeration.\n  \
         only in captured: {:?}\n  only in expected: {:?}",
        captured.difference(&expected).collect::<Vec<_>>(),
        expected.difference(&captured).collect::<Vec<_>>(),
    );
}
