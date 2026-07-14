//! Unit tests for the compiler-artifact inventory parser. Fixture lines
//! mirror real `cargo build --message-format=json` output.

#![allow(clippy::expect_used, clippy::panic)]

use super::*;

fn build_json(root: &str) -> String {
    [
        // Workspace bin deliverable.
        format!(
            r#"{{"reason":"compiler-artifact","package_id":"path+file://{root}/crates/cargo-evidence#0.1.5","target":{{"name":"cargo-evidence","kind":["bin"]}},"filenames":["{root}/target/release/cargo-evidence"]}}"#
        ),
        // Workspace lib deliverable (rlib + rmeta).
        format!(
            r#"{{"reason":"compiler-artifact","package_id":"path+file://{root}/crates/evidence-core#0.1.5","target":{{"name":"evidence_core","kind":["lib"]}},"filenames":["{root}/target/release/libevidence_core.rlib","{root}/target/release/deps/libevidence_core-abc.rmeta"]}}"#
        ),
        // A workspace build-script target — NOT a deliverable.
        format!(
            r#"{{"reason":"compiler-artifact","package_id":"path+file://{root}/crates/evidence-core#0.1.5","target":{{"name":"build-script-build","kind":["custom-build"]}},"filenames":["{root}/target/release/build/x/build-script-build"]}}"#
        ),
        // An external dependency — different package id root, excluded.
        r#"{"reason":"compiler-artifact","package_id":"registry+https://github.com/rust-lang/crates.io-index#serde@1.0","target":{"name":"serde","kind":["lib"]},"filenames":["/root/.cargo/registry/serde.rlib"]}"#.to_string(),
        // A non-artifact message line, ignored.
        r#"{"reason":"build-finished","success":true}"#.to_string(),
    ]
    .join("\n")
}

#[test]
fn keeps_only_workspace_lib_and_bin_deliverables() {
    let arts = parse_workspace_artifacts(&build_json("/work/repo"), Path::new("/work/repo"));
    let keys: Vec<&str> = arts.iter().map(|a| a.key.as_str()).collect();
    assert_eq!(
        keys,
        vec![
            "target/release/cargo-evidence",
            "target/release/deps/libevidence_core-abc.rmeta",
            "target/release/libevidence_core.rlib",
        ]
    );
}

#[test]
fn excludes_build_scripts_and_external_deps() {
    let arts = parse_workspace_artifacts(&build_json("/work/repo"), Path::new("/work/repo"));
    assert!(
        !arts.iter().any(|a| a.key.contains("build-script-build")),
        "build scripts are not deliverables"
    );
    assert!(
        !arts
            .iter()
            .any(|a| a.path.to_string_lossy().contains("registry")),
        "external deps are excluded"
    );
}

#[test]
fn artifact_path_is_absolute_and_key_is_workspace_relative() {
    let arts = parse_workspace_artifacts(&build_json("/work/repo"), Path::new("/work/repo"));
    let bin = arts
        .iter()
        .find(|a| a.key == "target/release/cargo-evidence")
        .expect("bin present");
    assert_eq!(
        bin.path,
        Path::new("/work/repo/target/release/cargo-evidence")
    );
}

#[test]
fn empty_or_junk_input_yields_no_artifacts() {
    assert!(parse_workspace_artifacts("", Path::new("/x")).is_empty());
    assert!(parse_workspace_artifacts("not json\n{}", Path::new("/x")).is_empty());
}
