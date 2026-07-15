//! Behavior tests for the override-haystack heuristic. The interesting
//! logic is `haystack_in_workflow` / `pr_body_feeds_override` — pure
//! predicates over one workflow file's text, so they are exercised
//! directly without a filesystem.

use super::*;

#[test]
fn commits_array_haystack_is_accepted() {
    let wf = "env:\n  X: ${{ toJSON(github.event.commits.*.message) }}\n";
    assert!(haystack_in_workflow(wf));
}

#[test]
fn pr_body_feeding_the_override_gate_is_accepted() {
    // PR body plumbed alongside the override marker — the PR-event gate
    // reads the override straight from the body.
    let wf = "env:\n  PR_BODY: ${{ github.event.pull_request.body }}\n\
              # gate reads Override-Deterministic-Baseline from PR_BODY\n";
    assert!(haystack_in_workflow(wf));
}

#[test]
fn pr_body_used_for_something_else_is_rejected() {
    // PR body referenced for an unrelated purpose (no override marker)
    // must NOT count as override-haystack plumbing.
    let wf = "run: gh pr comment --body \"${{ github.event.pull_request.body }}\"\n";
    assert!(!haystack_in_workflow(wf));
}

#[test]
fn pr_body_and_marker_in_unrelated_jobs_is_rejected() {
    // Body echoed in job `a`; the override marker only appears far away
    // in a comment in job `b`. Same file, different gates — the marker
    // does not govern the body reference, so this must NOT qualify.
    let mut wf = String::from(
        "jobs:\n  a:\n    steps:\n      - run: echo ${{ github.event.pull_request.body }}\n",
    );
    wf.push_str(&"      # filler line\n".repeat(60)); // >600 bytes apart
    wf.push_str("  b:\n    # Override-Deterministic-Baseline mentioned in an unrelated job\n");
    assert!(!haystack_in_workflow(&wf));
}

#[test]
fn neither_haystack_is_rejected() {
    assert!(!haystack_in_workflow("run: cargo test --workspace\n"));
}
