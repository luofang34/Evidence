//! Lock test for the cross-time-determinism CI contract.
//!
//! The cross-time gate's correctness is structural, not something a
//! unit test can exercise (it depends on GitHub event context and
//! cross-run artifacts). This guardrail pins the properties a
//! reviewer would otherwise have to re-verify by hand on every
//! `.github/workflows/ci.yml` edit:
//!
//! 1. `workflow_dispatch` stays enabled — the out-of-band trigger that
//!    republishes the baseline on the current `main` tip.
//! 2. The cross-time comparison runs for pull requests only (job-level
//!    `if: github.event_name == 'pull_request'`). Comparing on a `main`
//!    event would risk freezing the baseline at a stale run.
//! 3. Baseline publication belongs to `evidence-cross-host`: it uploads
//!    the `xhost-<os>` artifact that the cross-time job downloads on a
//!    later PR. Producer and consumer must name the same artifact.
//! 4. A failed comparison cannot be summarized as a match — the Summary
//!    step keys on the compare step's `outcome`, not merely on whether a
//!    prior artifact existed.
//!
//! Mechanical-guardrail style (no `Diagnostic`, no `RULES` entry —
//! mirrors `workflow_action_versions_locked` / `schema_versions_locked`).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use std::fs;
use std::path::PathBuf;

fn ci_yml() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("workspace root")
        .join(".github")
        .join("workflows")
        .join("ci.yml");
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e))
}

fn fetch_script() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("workspace root")
        .join("scripts")
        .join("cross-time-fetch-baseline.sh");
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e))
}

/// Extract the body of the top-level job `name` (a 2-space-indented
/// key) up to the next top-level job or EOF.
fn job_block<'a>(yml: &'a str, name: &str) -> &'a str {
    let header = format!("\n  {name}:");
    let start = yml
        .find(&header)
        .unwrap_or_else(|| panic!("job `{name}` not found in ci.yml"))
        + 1;
    let rest = &yml[start..];
    // End at the next line that starts a 2-space-indented key (`  x:`),
    // i.e. a sibling job. Skip the header line itself.
    let after_header = rest.find('\n').map(|i| i + 1).unwrap_or(rest.len());
    let mut end = rest.len();
    let mut idx = after_header;
    for line in rest[after_header..].split_inclusive('\n') {
        let is_sibling = line.len() >= 3
            && line.as_bytes()[0] == b' '
            && line.as_bytes()[1] == b' '
            && line.as_bytes()[2] != b' '
            && line.as_bytes()[2] != b'#';
        if is_sibling {
            end = idx;
            break;
        }
        idx += line.len();
    }
    &rest[..end]
}

#[test]
fn workflow_dispatch_is_enabled() {
    let yml = ci_yml();
    // `on:` trigger block. `workflow_dispatch:` must be present so the
    // baseline can be republished on demand.
    assert!(
        yml.contains("\n  workflow_dispatch:"),
        "ci.yml `on:` must keep `workflow_dispatch:` — the out-of-band \
         trigger that republishes the cross-time baseline on the current \
         main tip."
    );
}

#[test]
fn cross_time_compares_on_pull_requests_only() {
    let yml = ci_yml();
    let job = job_block(&yml, "cross-time-determinism");
    assert!(
        job.contains("if: github.event_name == 'pull_request'"),
        "the cross-time-determinism job must be gated \
         `if: github.event_name == 'pull_request'`; comparing on a main \
         event would freeze the baseline at a stale run."
    );
}

#[test]
fn baseline_fetch_uses_the_fail_closed_script() {
    // The fail-closed baseline fetch lives in a domain script whose
    // exit-code behavior is unit-tested (see the
    // `cross_time_fetch_baseline` integration test). Pin that the
    // workflow actually invokes it and doesn't reintroduce an inline
    // `prior_missing=1` soft-skip — a required check that skipped would
    // pass without ever comparing.
    let yml = ci_yml();
    let job = job_block(&yml, "cross-time-determinism");
    assert!(
        job.contains("cross-time-fetch-baseline.sh"),
        "the cross-time job must fetch the baseline via \
         `scripts/cross-time-fetch-baseline.sh` (its fail-closed \
         behavior is unit-tested there)."
    );
    assert!(
        !job.contains("prior_missing=1"),
        "the workflow must not reintroduce an inline `prior_missing=1` \
         soft-skip."
    );
}

#[test]
fn baseline_producer_and_consumer_agree_on_artifact() {
    let yml = ci_yml();
    // Producer: the cross-host job uploads one artifact per OS.
    assert!(
        yml.contains("name: xhost-${{ runner.os }}"),
        "evidence-cross-host must upload the `xhost-<os>` artifact that \
         is the cross-time baseline."
    );
    // Consumer: the fetch script downloads the Linux one.
    assert!(
        fetch_script().contains("--name xhost-Linux"),
        "the baseline-fetch script must download the `xhost-Linux` \
         artifact published by a successful main run."
    );
}

#[test]
fn baseline_lookup_is_bound_to_pr_base_sha() {
    // The baseline must be the run for the PR's exact base commit, not
    // merely the newest successful run — otherwise it drifts to a stale
    // historical commit. The workflow passes the base SHA into the
    // fetch script, which selects the run by `head_sha=<base sha>`.
    let yml = ci_yml();
    let job = job_block(&yml, "cross-time-determinism");
    assert!(
        job.contains("BASE_SHA: ${{ github.event.pull_request.base.sha }}"),
        "the workflow must pass the PR base SHA \
         (`github.event.pull_request.base.sha`) into the fetch script."
    );
    assert!(
        fetch_script().contains("head_sha=${BASE_SHA}"),
        "the fetch script must select the run by `head_sha=<base sha>` \
         so it resolves the baseline for the exact base commit."
    );
}

#[test]
fn override_is_read_from_a_merge_style_immune_haystack() {
    // A PR-only cross-time gate reads the `Override-Deterministic-Baseline`
    // line from `github.event.pull_request.body` — always present on the
    // pull_request event, so merge style can never hide it. The doctor
    // `check_merge_style` self-check (exercised by the cert-profile CI
    // job) treats this as the mitigation ONLY when the PR body co-occurs
    // with the override marker; removing either fails cert generation for
    // a non-obvious reason, so pin the pair here where the failure is
    // fast and self-explanatory.
    let yml = ci_yml();
    let commits_haystack = yml.contains("github.event.commits");
    let pr_body_gate = yml.contains("github.event.pull_request.body")
        && yml.contains("Override-Deterministic-Baseline");
    assert!(
        commits_haystack || pr_body_gate,
        "the cross-time gate must read the override from a merge-style-\
         immune haystack: `github.event.pull_request.body` paired with \
         the `Override-Deterministic-Baseline` marker, or a workflow \
         plumbing `github.event.commits` — otherwise \
         `cargo evidence doctor`'s merge-style check flips to a warning \
         and cert-profile generation fails."
    );
}

#[test]
fn failed_comparison_cannot_report_a_match() {
    let yml = ci_yml();
    let job = job_block(&yml, "cross-time-determinism");
    // The compare step is addressable and the Summary keys on its
    // outcome — otherwise a failed drift-lint could still print "matched".
    assert!(
        job.contains("id: compare"),
        "the drift-comparison step must carry `id: compare` so the \
         Summary can read its outcome."
    );
    // The "matched" verdict must be gated on the compare step actually
    // succeeding — the exact `== 'success'` condition, not merely that
    // `steps.compare.outcome` is referenced somewhere. A skipped or
    // errored compare must not read as a match.
    assert!(
        job.contains("steps.compare.outcome }}\" = \"success\""),
        "the Summary must report a match only under \
         `steps.compare.outcome == 'success'`; any weaker condition lets \
         a skipped/errored compare masquerade as a match."
    );
}
