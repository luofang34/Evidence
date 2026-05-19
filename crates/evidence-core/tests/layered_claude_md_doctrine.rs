//! Gate the lean-layered `CLAUDE.md` doctrine (LLR-079).
//!
//! Every workspace crate under `crates/` must ship a per-crate
//! `CLAUDE.md` carrying local conventions. The gate asserts:
//!
//! 1. The file exists.
//! 2. It is non-trivial (>= 10 non-blank lines so a stub doesn't pass).
//! 3. It mentions its own scoped test command
//!    (`cargo test -p <crate-name>`) — the article's per-subdirectory
//!    test-scoping anti-pattern fix.
//! 4. It is under 80 lines total so the file stays "local conventions
//!    only," not a re-statement of root rules. (Article: "lean,
//!    layered" — root for big picture, subdir for local.)
//!
//! Adding a new workspace crate without a `CLAUDE.md` is a hard fail.
//! Failure messages name the crate so the fix is obvious.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use std::fs;
use std::path::PathBuf;

use walkdir::WalkDir;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

fn crate_dirs(root: &PathBuf) -> Vec<(String, PathBuf)> {
    let crates_root = root.join("crates");
    WalkDir::new(&crates_root)
        .follow_links(false)
        .min_depth(1)
        .max_depth(1)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_dir())
        .map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            (name, e.into_path())
        })
        .collect()
}

#[test]
fn every_workspace_crate_has_lean_layered_claude_md() {
    let root = workspace_root();
    let mut failures: Vec<String> = Vec::new();

    for (name, dir) in crate_dirs(&root) {
        let path = dir.join("CLAUDE.md");
        if !path.exists() {
            failures.push(format!("crates/{name}/CLAUDE.md is missing"));
            continue;
        }
        let body = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read crates/{name}/CLAUDE.md: {e}"));
        let lines: Vec<&str> = body.lines().collect();
        let non_blank = lines.iter().filter(|l| !l.trim().is_empty()).count();
        if non_blank < 10 {
            failures.push(format!(
                "crates/{name}/CLAUDE.md is too thin ({non_blank} non-blank lines; need >= 10)"
            ));
        }
        if lines.len() > 80 {
            failures.push(format!(
                "crates/{name}/CLAUDE.md is {} lines (cap is 80; trim local conventions or move project-wide rules to root)",
                lines.len()
            ));
        }
        let needle = format!("cargo test -p {name}");
        if !body.contains(&needle) {
            failures.push(format!(
                "crates/{name}/CLAUDE.md must document its scoped test command: `{needle}`"
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "lean-layered CLAUDE.md doctrine violations:\n  - {}",
        failures.join("\n  - ")
    );
}
