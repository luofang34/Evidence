//! Unit tests for in-scope package resolution and input planning.

#![allow(clippy::expect_used, clippy::panic)]

use super::*;

/// Minimal `cargo metadata --format-version 1` JSON: a package whose
/// name (`evidence-core`) differs from its directory (`crates/…`),
/// which is exactly the #138 mismatch.
fn metadata_json(root: &str) -> String {
    format!(
        r#"{{
            "workspace_root": "{root}",
            "packages": [
                {{ "name": "evidence-core",  "manifest_path": "{root}/crates/evidence-core/Cargo.toml" }},
                {{ "name": "cargo-evidence", "manifest_path": "{root}/crates/cargo-evidence/Cargo.toml" }},
                {{ "name": "unrelated",      "manifest_path": "{root}/vendor/unrelated/Cargo.toml" }}
            ]
        }}"#
    )
}

#[test]
fn resolves_package_name_to_manifest_dir() {
    let json = metadata_json("/work/repo");
    let units = resolve_in_scope_units(&json, &["evidence-core".into(), "cargo-evidence".into()])
        .expect("resolves");
    assert_eq!(
        units,
        vec![
            ResolvedUnit {
                name: "evidence-core".into(),
                rel_dir: "crates/evidence-core".into(),
            },
            ResolvedUnit {
                name: "cargo-evidence".into(),
                rel_dir: "crates/cargo-evidence".into(),
            },
        ]
    );
}

#[test]
fn missing_package_fails_closed() {
    let json = metadata_json("/work/repo");
    let err = resolve_in_scope_units(&json, &["does-not-exist".into()])
        .expect_err("must reject unknown package");
    assert!(matches!(err, InputScopeError::MissingPackage { name } if name == "does-not-exist"));
}

#[test]
fn manifest_outside_workspace_is_path_escape() {
    // `unrelated` sits under `/work/repo/vendor`, still under root —
    // so use a root that the manifest is NOT under to force escape.
    let json = metadata_json("/work/repo");
    let err = resolve_in_scope_units(
        &json.replace(
            "\"workspace_root\": \"/work/repo\"",
            "\"workspace_root\": \"/other/root\"",
        ),
        &["evidence-core".into()],
    )
    .expect_err("manifest not under root escapes");
    assert!(matches!(err, InputScopeError::PathEscape { name, .. } if name == "evidence-core"));
}

#[test]
fn parse_error_surfaces() {
    let err = resolve_in_scope_units("not json", &["evidence-core".into()]).expect_err("bad json");
    assert!(matches!(err, InputScopeError::ParseMetadata(_)));
}

#[test]
fn plan_dedupes_and_tags_provenance() {
    let unit = ResolvedUnit {
        name: "evidence-core".into(),
        rel_dir: "crates/evidence-core".into(),
    };
    let unit_files = vec![(
        unit,
        vec![
            "crates/evidence-core/src/lib.rs".to_string(),
            "crates/evidence-core/Cargo.toml".to_string(),
        ],
    )];
    let required = vec![
        // A generated (git-ignored) input declared required in
        // boundary.toml — must appear tagged DeclaredRequired.
        "target/generated/table.rs".to_string(),
    ];
    let control = vec![
        "Cargo.lock".to_string(),
        "cert/boundary.toml".to_string(),
        // Overlap with a unit file: unit provenance must win.
        "crates/evidence-core/Cargo.toml".to_string(),
    ];
    let plan = assemble_input_plan(&unit_files, &required, &control).expect("plan");
    // Sorted, deduped: 5 distinct paths.
    let paths: Vec<&str> = plan.iter().map(|e| e.path.as_str()).collect();
    assert_eq!(
        paths,
        vec![
            "Cargo.lock",
            "cert/boundary.toml",
            "crates/evidence-core/Cargo.toml",
            "crates/evidence-core/src/lib.rs",
            "target/generated/table.rs",
        ]
    );
    let overlap = plan
        .iter()
        .find(|e| e.path == "crates/evidence-core/Cargo.toml")
        .expect("present");
    assert_eq!(
        overlap.reason,
        InputReason::InScopeUnit("evidence-core".into())
    );
    let control_entry = plan
        .iter()
        .find(|e| e.path == "Cargo.lock")
        .expect("present");
    assert_eq!(control_entry.reason, InputReason::WorkspaceControl);
    let required_entry = plan
        .iter()
        .find(|e| e.path == "target/generated/table.rs")
        .expect("present");
    assert_eq!(required_entry.reason, InputReason::DeclaredRequired);
}

#[test]
fn empty_unit_fails_closed() {
    let unit = ResolvedUnit {
        name: "evidence-core".into(),
        rel_dir: "crates/evidence-core".into(),
    };
    let err = assemble_input_plan(&[(unit, vec![])], &[], &["Cargo.lock".into()])
        .expect_err("a unit with zero files must fail");
    assert!(matches!(err, InputScopeError::EmptyScope { name, .. } if name == "evidence-core"));
}

#[test]
fn zero_total_inputs_fails_closed() {
    let err = assemble_input_plan(&[], &[], &[]).expect_err("no inputs at all must fail");
    assert!(matches!(err, InputScopeError::NoInputs));
}

#[test]
fn walk_fallback_captures_files_and_excludes_target_and_git() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::create_dir_all(root.join("crates/pkg/src")).expect("mkdir");
    std::fs::write(root.join("crates/pkg/src/lib.rs"), b"// src").expect("w");
    std::fs::write(root.join("crates/pkg/Cargo.toml"), b"[package]").expect("w");
    std::fs::write(root.join("Cargo.toml"), b"[workspace]").expect("w");
    // Must be excluded:
    std::fs::create_dir_all(root.join("crates/pkg/target/debug")).expect("mkdir");
    std::fs::write(root.join("crates/pkg/target/debug/pkg"), b"binary").expect("w");
    std::fs::create_dir_all(root.join(".git")).expect("mkdir");
    std::fs::write(root.join(".git/HEAD"), b"ref").expect("w");

    // A dir pathspec walks; a file pathspec includes just that file.
    let files = walk_pathspecs(root, &["crates/pkg", "Cargo.toml"]).expect("walk");
    assert_eq!(
        files,
        vec![
            "Cargo.toml".to_string(),
            "crates/pkg/Cargo.toml".to_string(),
            "crates/pkg/src/lib.rs".to_string(),
        ],
        "target/ and .git/ excluded; source captured, sorted"
    );
}

#[test]
fn required_input_presence_is_checked() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(dir.path().join("gen")).expect("mkdir");
    std::fs::write(dir.path().join("gen/present.rs"), b"// generated").expect("write");

    // A declared input that exists passes.
    check_required_inputs_exist(dir.path(), &["gen/present.rs".to_string()]).expect("present ok");

    // A declared-but-absent input fails closed.
    let err = check_required_inputs_exist(dir.path(), &["gen/missing.rs".to_string()])
        .expect_err("missing required input must fail");
    assert!(
        matches!(err, InputScopeError::MissingRequiredInput { path } if path == "gen/missing.rs")
    );
}

#[test]
fn required_input_absolute_or_parent_escape_is_rejected() {
    let dir = tempfile::tempdir().expect("tempdir");
    for bad in ["/etc/passwd", "../outside.rs", "crates/../../escape.rs"] {
        let err = check_required_inputs_exist(dir.path(), &[bad.to_string()])
            .expect_err("escaping required input must be rejected");
        assert!(
            matches!(err, InputScopeError::RequiredInputEscape { .. }),
            "{bad} should be RequiredInputEscape, got {err:?}"
        );
    }
}

fn run_git(dir: &Path, args: &[&str]) {
    let ok = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("spawn git")
        .status
        .success();
    assert!(ok, "git {args:?} failed");
}

#[test]
fn enumerate_uses_git_inside_a_worktree() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    run_git(root, &["init", "-q"]);
    std::fs::create_dir_all(root.join("src")).expect("mkdir");
    std::fs::write(root.join("src/tracked.rs"), b"// tracked").expect("w");
    std::fs::write(root.join("src/untracked.rs"), b"// untracked").expect("w");
    run_git(root, &["add", "src/tracked.rs"]);

    assert!(is_git_worktree(root));
    // Git path: only the tracked file — a filesystem walk would also
    // return the untracked one.
    let files = enumerate_pathspecs(root, &["src"]).expect("enumerate");
    assert_eq!(files, vec!["src/tracked.rs".to_string()]);
}

#[test]
fn enumerate_walks_only_when_not_a_worktree() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::create_dir_all(root.join("src")).expect("mkdir");
    std::fs::write(root.join("src/a.rs"), b"a").expect("w");
    std::fs::write(root.join("src/b.rs"), b"b").expect("w");

    assert!(!is_git_worktree(root));
    let files = enumerate_pathspecs(root, &["src"]).expect("enumerate");
    assert_eq!(files, vec!["src/a.rs".to_string(), "src/b.rs".to_string()]);
}

#[test]
fn git_failure_inside_a_worktree_propagates_instead_of_walking() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    run_git(root, &["init", "-q"]);
    std::fs::write(root.join("f.rs"), b"x").expect("w");
    run_git(root, &["add", "f.rs"]);
    // Corrupt the index so `git ls-files` fails while the dir is still a
    // working tree: the error must propagate, not silently fall back.
    std::fs::write(root.join(".git/index"), b"not a valid git index").expect("corrupt");
    assert!(
        is_git_worktree(root),
        "still a worktree after index corruption"
    );
    let err = enumerate_pathspecs(root, &["."]).expect_err("git failure must propagate");
    assert!(matches!(err, InputScopeError::GitLsFiles(_)), "got {err:?}");
}
