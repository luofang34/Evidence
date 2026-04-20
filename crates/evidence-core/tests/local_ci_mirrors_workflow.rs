//! Meta self-check: `scripts/local-ci.sh` must contain every cargo
//! gate run by the `check` job in `.github/workflows/ci.yml`.
//!
//! The class of bug this test prevents: a partial `RUSTDOCFLAGS`
//! locally (`-D missing_docs`) passes while CI's full
//! `RUSTDOCFLAGS` (`-D rustdoc::broken_intra_doc_links`) catches
//! an intra-doc link the local run couldn't see. A single local
//! script that mirrors CI exactly, plus this test to keep them in
//! sync, closes the loop.
//!
//! The check is grep-level — we don't parse YAML or bash. For each
//! cargo invocation in the `check` job's steps, assert a matching
//! substring appears in `scripts/local-ci.sh`. Additions to CI that
//! miss the script fire here before merge.
//!
//! Out of scope: matching non-cargo steps (Linux-only grep gates
//! like the schema-literal scan or the dead-code delta gate). Those
//! run in CI on every push and don't belong in a pre-push loop
//! (they're base-branch-relative and would gate local pushes on git
//! history rather than tree state).
//!
//! ## Known limits of substring matching
//!
//! `.contains(token)` is a cheap structural check that accepts false
//! greens: a commented-out `# cargo test --workspace` in the YAML
//! would still satisfy `cargo test --workspace`, and `-D warnings`
//! appears in multiple contexts (RUSTFLAGS env + clippy args), so
//! deleting the clippy line while RUSTFLAGS stays set would not
//! fire. Acceptable today because both files are short (~50-150
//! lines) and any deletion is reviewer-visible in the diff. **If
//! either file grows past ~200 lines, upgrade this test** to parse
//! the YAML `steps:` array and the script's executed commands
//! (shellcheck-style) instead of substring scanning.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use std::fs;
use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

/// The exact cargo gates the `check` job runs. Each entry is a
/// substring that must appear verbatim in both ci.yml and
/// scripts/local-ci.sh. Additions here require a change in both
/// files; the test fires on any PR that adds a CI gate without
/// mirroring it locally.
///
/// Format contract: each string is a short, stable token of the
/// cargo invocation — enough to uniquely identify the gate, not the
/// full command line (so quoting / line-wrapping differences between
/// YAML and bash don't matter).
const REQUIRED_GATES: &[&str] = &[
    // fmt
    "cargo fmt --all",
    // clippy
    "cargo clippy --workspace --all-targets",
    "-D warnings",
    // test
    "cargo test --workspace",
    // doc gate
    "cargo doc --workspace --no-deps",
    "-D rustdoc::broken_intra_doc_links",
    "-D rustdoc::private_intra_doc_links",
    // release build
    "cargo build --workspace --release",
];

#[test]
fn local_ci_script_mirrors_workflow() {
    let root = workspace_root();
    let script_path = root.join("scripts").join("local-ci.sh");
    let workflow_path = root.join(".github").join("workflows").join("ci.yml");

    let script = fs::read_to_string(&script_path)
        .unwrap_or_else(|e| panic!("reading {}: {}", script_path.display(), e));
    let workflow = fs::read_to_string(&workflow_path)
        .unwrap_or_else(|e| panic!("reading {}: {}", workflow_path.display(), e));

    let mut missing_from_script: Vec<&str> = Vec::new();
    let mut missing_from_workflow: Vec<&str> = Vec::new();
    for &gate in REQUIRED_GATES {
        if !script.contains(gate) {
            missing_from_script.push(gate);
        }
        if !workflow.contains(gate) {
            missing_from_workflow.push(gate);
        }
    }

    assert!(
        missing_from_script.is_empty() && missing_from_workflow.is_empty(),
        "scripts/local-ci.sh and .github/workflows/ci.yml must both contain every \
         gate in REQUIRED_GATES.\n\
         missing from scripts/local-ci.sh: {:?}\n\
         missing from .github/workflows/ci.yml: {:?}\n\n\
         if CI added a new cargo gate, add it to both places and to REQUIRED_GATES. \
         if CI removed a gate, drop it from all three.",
        missing_from_script,
        missing_from_workflow
    );
}

/// Executable bit check — a committed shell script that loses its
/// `chmod +x` silently stops being invokable from `pre-push`. Unix-
/// only; Windows doesn't honor the bit.
#[cfg(unix)]
#[test]
fn local_ci_script_is_executable() {
    use std::os::unix::fs::PermissionsExt;
    let path = workspace_root().join("scripts").join("local-ci.sh");
    let meta = fs::metadata(&path).unwrap_or_else(|e| panic!("stat {}: {}", path.display(), e));
    let mode = meta.permissions().mode();
    assert!(
        mode & 0o111 != 0,
        "{} must be executable (current mode: {:o}); \
         run `chmod +x scripts/local-ci.sh` and commit the permission",
        path.display(),
        mode
    );
}

/// Same for the pre-push hook.
#[cfg(unix)]
#[test]
fn pre_push_hook_is_executable() {
    use std::os::unix::fs::PermissionsExt;
    let path = workspace_root().join(".githooks").join("pre-push");
    let meta = fs::metadata(&path).unwrap_or_else(|e| panic!("stat {}: {}", path.display(), e));
    let mode = meta.permissions().mode();
    assert!(
        mode & 0o111 != 0,
        "{} must be executable (current mode: {:o}); \
         run `chmod +x .githooks/pre-push` and commit the permission",
        path.display(),
        mode
    );
}
