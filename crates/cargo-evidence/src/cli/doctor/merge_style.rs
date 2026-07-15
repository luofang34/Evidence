//! `check_merge_style` — can an `Override-Deterministic-Baseline:` line
//! reach the gate that reads it? (LLR-048.)
//!
//! Split from `checks.rs` to stay under the 500-line workspace limit and
//! to give the override-haystack heuristic a home for its behavior
//! tests. Emits `DOCTOR_MERGE_STYLE_RISK` / `DOCTOR_MERGE_STYLE_UNKNOWN`.

use std::path::Path;
use std::process::Command;

use super::CheckResult;

/// The override-line marker a gate reads from its haystack.
const OVERRIDE_MARKER: &str = "Override-Deterministic-Baseline";

pub(super) fn check_merge_style(workspace: &Path) -> CheckResult {
    // The real question: can an `Override-Deterministic-Baseline:` line
    // in a PR body actually reach the gate that reads it? Two
    // mitigations suffice:
    //   (a) a workflow reads the override from a merge-style-immune
    //       haystack — `github.event.commits[*].message` (push-event) or
    //       `github.event.pull_request.body` (a PR-only gate reads the
    //       PR body directly, so merge style is irrelevant)
    //   (b) repo uses squash-merge exclusively (PR body lands in the
    //       squashed head_commit message)
    // If EITHER is in place, no warning. If NEITHER, merge-commit
    // history points at a real gap.

    // (a) Workflow-plumb probe.
    if workflow_plumbs_override_haystack(workspace) {
        return CheckResult::Pass;
    }

    // (b) History probe — count merge-commits in recent history
    //     of the default branch. Try `main` first (modern default),
    //     fall back to `master` (older repos). If neither resolves,
    //     fire UNKNOWN.
    let stdout = match git_log_default_branch(workspace) {
        Ok(s) => s,
        Err(e) => {
            return CheckResult::Fail("DOCTOR_MERGE_STYLE_UNKNOWN", e);
        }
    };
    let lines: Vec<&str> = stdout.lines().collect();
    if lines.is_empty() {
        return CheckResult::Pass;
    }
    let merge_commits = lines
        .iter()
        .filter(|l| l.starts_with("Merge pull request #"))
        .count();
    if merge_commits == 0 {
        return CheckResult::Pass; // all-squash history.
    }
    CheckResult::Fail(
        "DOCTOR_MERGE_STYLE_RISK",
        format!(
            "{}/{} recent main commits are merge-commits (`Merge pull \
             request #`), AND no workflow reads the override from a \
             merge-style-immune haystack. On push-to-main events a \
             gate that reads only `head_commit.message` sees the \
             mechanical merge-commit subject — so an Override-\
             Deterministic-Baseline line in the PR body never surfaces. \
             Mitigations: (a) switch to squash-merge in repo Settings → \
             General → Pull Requests; (b) read the override on the \
             pull_request event from `github.event.pull_request.body`, \
             or plumb `github.event.commits[*].message` (see \
             cargo-evidence's own ci.yml).",
            merge_commits,
            lines.len()
        ),
    )
}

/// Run `git log -n 20 --format=%s` against the repo's default
/// branch. Tries `main` then `master`; if both fail, returns a
/// descriptive error (surfaced as `DOCTOR_MERGE_STYLE_UNKNOWN`).
fn git_log_default_branch(workspace: &Path) -> Result<String, String> {
    let candidates = ["main", "master"];
    let mut last_err: Option<String> = None;
    for branch in &candidates {
        let out = Command::new("git")
            .args(["log", "-n", "20", "--format=%s", branch])
            .current_dir(workspace)
            .output();
        match out {
            Ok(o) if o.status.success() => {
                return Ok(String::from_utf8_lossy(&o.stdout).into_owned());
            }
            Ok(o) => {
                last_err = Some(format!(
                    "`git log {}` exited non-zero (code {:?})",
                    branch,
                    o.status.code()
                ));
            }
            Err(e) => {
                return Err(format!(
                    "git unavailable ({}); merge-style policy could not be audited. \
                     Install git or re-run doctor in a repo clone.",
                    e
                ));
            }
        }
    }
    Err(format!(
        "neither `main` nor `master` branch is available ({}); merge-style \
         policy could not be audited. This repo either uses a non-standard \
         default branch name or has no main-line history yet.",
        last_err.unwrap_or_else(|| "no git output captured".to_string())
    ))
}

/// True iff some workflow reads the override from a merge-style-immune
/// haystack. See [`haystack_in_workflow`] for what qualifies.
fn workflow_plumbs_override_haystack(workspace: &Path) -> bool {
    let wf_dir = workspace.join(".github").join("workflows");
    if !wf_dir.is_dir() {
        return false;
    }
    walkdir::WalkDir::new(&wf_dir)
        .follow_links(false)
        .max_depth(2)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .filter(|e| {
            matches!(
                e.path().extension().and_then(|x| x.to_str()),
                Some("yml") | Some("yaml")
            )
        })
        .any(|e| {
            std::fs::read_to_string(e.path())
                .map(|t| haystack_in_workflow(&t))
                .unwrap_or(false)
        })
}

/// Pure predicate over one workflow file's text. Two forms qualify as
/// merge-style-immune override plumbing:
///
/// - `github.event.commits[*].message`: a push-event haystack that
///   survives any merge style; or
/// - `github.event.pull_request.body` used *as the override haystack* —
///   it must co-occur with the `Override-Deterministic-Baseline` marker,
///   so an unrelated PR-body reference (posting a comment, say) does not
///   masquerade as override plumbing.
fn haystack_in_workflow(text: &str) -> bool {
    if text.contains("github.event.commits") {
        return true;
    }
    text.contains("github.event.pull_request.body") && text.contains(OVERRIDE_MARKER)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commits_array_haystack_is_accepted() {
        let wf = "env:\n  X: ${{ toJSON(github.event.commits.*.message) }}\n";
        assert!(haystack_in_workflow(wf));
    }

    #[test]
    fn pr_body_feeding_the_override_gate_is_accepted() {
        // PR body plumbed alongside the override marker — the PR-event
        // gate reads the override straight from the body.
        let wf = "env:\n  PR_BODY: ${{ github.event.pull_request.body }}\n\
                  # gate reads Override-Deterministic-Baseline from PR_BODY\n";
        assert!(haystack_in_workflow(wf));
    }

    #[test]
    fn pr_body_used_for_something_else_is_rejected() {
        // PR body referenced for an unrelated purpose (no override
        // marker) must NOT count as override-haystack plumbing.
        let wf = "run: gh pr comment --body \"${{ github.event.pull_request.body }}\"\n";
        assert!(!haystack_in_workflow(wf));
    }

    #[test]
    fn neither_haystack_is_rejected() {
        assert!(!haystack_in_workflow("run: cargo test --workspace\n"));
    }
}
