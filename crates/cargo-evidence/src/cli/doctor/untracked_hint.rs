//! Slack-message refinement: when `DOCTOR_FLOORS_SLACK` fires on a
//! `test_count` dimension, point the user at untracked `.rs` files in
//! the working tree before they spend time investigating from
//! scratch. Editor duplicates (`* 2.rs`), `cp` artifacts, and
//! save-as accidents are the most common cause of test_count slack
//! that doesn't reproduce on a clean checkout (CI sees the tree
//! that's actually committed; doctor walks the live filesystem).
//!
//! Module-private to `doctor`; tested inline against a tempdir
//! `git init` repo. The helper degrades gracefully when `git` is
//! unavailable or the workspace isn't a repo — both return the
//! original slack message unchanged.

use std::path::Path;
use std::process::Command;

/// Compose the `DOCTOR_FLOORS_SLACK` user-facing message. When the
/// slack list contains any `test_count` entry AND the working tree
/// has untracked `.rs` files, append a hint pointing at them.
/// Otherwise return the base message verbatim.
pub(super) fn slack_message_with_hint(workspace: &Path, slack: &[String]) -> String {
    let mut msg = format!(
        "current measurement > committed floor on {} dimension(s). The project's \
         internal `floors_equal_current_no_slack` test enforces strict equality; \
         downstream adopters should mirror it. Raise the floor to close the gap: {}",
        slack.len(),
        slack.join("; ")
    );
    if !slack.iter().any(|s| s.contains("test_count")) {
        return msg;
    }
    let untracked = untracked_rs_files(workspace);
    if untracked.is_empty() {
        return msg;
    }
    let preview: Vec<&str> = untracked.iter().take(5).map(String::as_str).collect();
    let suffix = if untracked.len() > preview.len() {
        format!(" (and {} more)", untracked.len() - preview.len())
    } else {
        String::new()
    };
    msg.push_str(&format!(
        " | hint: working tree has {} untracked .rs file(s) which may be inflating \
         the count without entering git: {}{}. Delete with `\\rm` (the leading \
         backslash bypasses shell aliases) and re-run, or add to .gitignore if \
         intentional.",
        untracked.len(),
        preview.join(", "),
        suffix
    ));
    msg
}

/// `.rs` files present on disk but untracked by git. Workspace-
/// relative paths with forward-slash separators on every platform
/// (per `project_path_separator_on_windows`). Empty when `git` is
/// unavailable, the dir isn't a git repo, or there are no untracked
/// `.rs` files.
fn untracked_rs_files(workspace: &Path) -> Vec<String> {
    let out = Command::new("git")
        .args(["ls-files", "--others", "--exclude-standard"])
        .current_dir(workspace)
        .output();
    let stdout = match out {
        Ok(o) if o.status.success() => o.stdout,
        _ => return Vec::new(),
    };
    String::from_utf8_lossy(&stdout)
        .lines()
        .filter(|line| line.ends_with(".rs"))
        .map(|line| line.replace('\\', "/"))
        .collect()
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn init_git_repo(dir: &Path) {
        let out = Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(dir)
            .output()
            .expect("git init");
        assert!(out.status.success(), "git init failed: {:?}", out);
    }

    #[test]
    fn untracked_rs_files_finds_untracked() {
        let tmp = TempDir::new().unwrap();
        init_git_repo(tmp.path());
        fs::write(tmp.path().join("orphan.rs"), "// untracked\n").unwrap();
        fs::write(tmp.path().join("notes.md"), "ignored\n").unwrap();

        let hits = untracked_rs_files(tmp.path());
        assert_eq!(hits, vec!["orphan.rs".to_string()]);
    }

    #[test]
    fn untracked_rs_files_returns_empty_on_non_repo() {
        let tmp = TempDir::new().unwrap();
        // No `git init` — directory is not a git repo.
        fs::write(tmp.path().join("orphan.rs"), "// untracked\n").unwrap();
        let hits = untracked_rs_files(tmp.path());
        assert!(
            hits.is_empty(),
            "non-repo should return empty; got {hits:?}"
        );
    }

    #[test]
    fn slack_message_passes_through_when_no_test_count_dimension() {
        let tmp = TempDir::new().unwrap();
        init_git_repo(tmp.path());
        fs::write(tmp.path().join("orphan.rs"), "// untracked\n").unwrap();

        let slack = vec!["diagnostic_codes current=5 floor=4".to_string()];
        let msg = slack_message_with_hint(tmp.path(), &slack);
        assert!(
            !msg.contains("hint:"),
            "non-test_count slack should not get hint; got: {msg}"
        );
    }

    #[test]
    fn slack_message_appends_hint_on_test_count_with_untracked() {
        let tmp = TempDir::new().unwrap();
        init_git_repo(tmp.path());
        fs::write(tmp.path().join("orphan.rs"), "// untracked\n").unwrap();

        let slack = vec!["evidence-core/test_count current=10 floor=9".to_string()];
        let msg = slack_message_with_hint(tmp.path(), &slack);
        assert!(msg.contains("hint:"), "expected hint; got: {msg}");
        assert!(
            msg.contains("orphan.rs"),
            "expected orphan.rs in hint; got: {msg}"
        );
    }

    #[test]
    fn slack_message_no_hint_when_test_count_but_no_untracked() {
        let tmp = TempDir::new().unwrap();
        init_git_repo(tmp.path());
        // No untracked .rs files in this tree.

        let slack = vec!["evidence-core/test_count current=10 floor=9".to_string()];
        let msg = slack_message_with_hint(tmp.path(), &slack);
        assert!(!msg.contains("hint:"), "no untracked → no hint; got: {msg}");
    }

    #[test]
    fn slack_message_truncates_preview_above_five_files() {
        let tmp = TempDir::new().unwrap();
        init_git_repo(tmp.path());
        for i in 0..7 {
            fs::write(tmp.path().join(format!("orphan_{i}.rs")), "// untracked\n").unwrap();
        }

        let slack = vec!["evidence-core/test_count current=17 floor=10".to_string()];
        let msg = slack_message_with_hint(tmp.path(), &slack);
        assert!(
            msg.contains("(and 2 more)"),
            "expected truncation hint; got: {msg}"
        );
    }
}
