//! Unit tests for the compiler-artifact inventory parser. Fixture lines
//! mirror real `cargo build --message-format=json` output.

#![allow(clippy::expect_used, clippy::panic)]

use super::*;

fn members() -> BTreeSet<String> {
    [
        "path+file:///work/repo/crates/cargo-evidence#0.1.5".to_string(),
        "path+file:///work/repo/crates/evidence-core#0.1.5".to_string(),
    ]
    .into_iter()
    .collect()
}

fn build_json() -> String {
    [
        // Workspace-member bin deliverable.
        r#"{"reason":"compiler-artifact","package_id":"path+file:///work/repo/crates/cargo-evidence#0.1.5","target":{"name":"cargo-evidence","kind":["bin"]},"filenames":["/work/repo/target/release/cargo-evidence"]}"#,
        // Workspace-member lib deliverable (rlib + rmeta).
        r#"{"reason":"compiler-artifact","package_id":"path+file:///work/repo/crates/evidence-core#0.1.5","target":{"name":"evidence_core","kind":["lib"]},"filenames":["/work/repo/target/release/libevidence_core.rlib","/work/repo/target/release/deps/libevidence_core-abc.rmeta"]}"#,
        // A workspace-member build script — NOT a deliverable.
        r#"{"reason":"compiler-artifact","package_id":"path+file:///work/repo/crates/evidence-core#0.1.5","target":{"name":"build-script-build","kind":["custom-build"]},"filenames":["/work/repo/target/release/build/x/build-script-build"]}"#,
        // An external dependency — package_id is not a workspace member.
        r#"{"reason":"compiler-artifact","package_id":"registry+https://github.com/rust-lang/crates.io-index#serde@1.0","target":{"name":"serde","kind":["lib"]},"filenames":["/root/.cargo/registry/serde.rlib"]}"#,
        // A non-artifact message, ignored.
        r#"{"reason":"build-finished","success":true}"#,
    ]
    .join("\n")
}

#[test]
fn keeps_only_workspace_member_lib_and_bin_keyed_under_target_dir() {
    let arts = parse_workspace_artifacts(&build_json(), &members(), Path::new("/work/repo/target"))
        .expect("parse");
    let keys: Vec<&str> = arts.iter().map(|a| a.key.as_str()).collect();
    assert_eq!(
        keys,
        vec![
            "release/cargo-evidence",
            "release/deps/libevidence_core-abc.rmeta",
            "release/libevidence_core.rlib",
        ],
        "keys are relative to target_directory, sorted"
    );
}

#[test]
fn excludes_build_scripts_and_non_member_deps() {
    let arts = parse_workspace_artifacts(&build_json(), &members(), Path::new("/work/repo/target"))
        .expect("parse");
    assert!(!arts.iter().any(|a| a.key.contains("build-script")));
    assert!(
        !arts
            .iter()
            .any(|a| a.path.to_string_lossy().contains("registry")),
        "a package id that is not an exact workspace member is excluded"
    );
}

#[test]
fn malformed_message_is_a_typed_error() {
    let err = parse_workspace_artifacts("not json at all", &members(), Path::new("/t"))
        .expect_err("malformed line must fail, not silently skip");
    assert!(
        matches!(err, ArtifactError::MalformedMessage { .. }),
        "{err:?}"
    );
}

#[test]
fn artifact_outside_target_dir_is_rejected() {
    let line = r#"{"reason":"compiler-artifact","package_id":"path+file:///work/repo/crates/cargo-evidence#0.1.5","target":{"name":"cargo-evidence","kind":["bin"]},"filenames":["/elsewhere/cargo-evidence"]}"#;
    let err = parse_workspace_artifacts(line, &members(), Path::new("/work/repo/target"))
        .expect_err("artifact outside target_dir must be rejected, not keyed absolute");
    assert!(
        matches!(err, ArtifactError::ArtifactOutsideTargetDir { .. }),
        "{err:?}"
    );
}

#[test]
fn empty_or_blank_input_yields_no_artifacts() {
    assert!(
        parse_workspace_artifacts("", &members(), Path::new("/t"))
            .expect("ok")
            .is_empty()
    );
    assert!(
        parse_workspace_artifacts("\n   \n", &members(), Path::new("/t"))
            .expect("ok")
            .is_empty()
    );
}

#[test]
fn dev_build_args_are_a_default_debug_build() {
    // Dev must not pin the dependency graph or force release — fast
    // iteration is the point; provenance strictness is cert/record's job.
    let args = build_args(Profile::Dev);
    assert_eq!(args, vec!["build", "--workspace", "--message-format=json"]);
    assert!(!args.contains(&"--release"));
    assert!(!args.contains(&"--locked"));
}

#[test]
fn cert_and_record_build_args_are_release_locked() {
    for p in [Profile::Cert, Profile::Record] {
        let args = build_args(p);
        assert!(args.contains(&"--release"), "{p:?} must build --release");
        assert!(
            args.contains(&"--locked"),
            "{p:?} must build --locked (pinned dependency graph)"
        );
    }
}
