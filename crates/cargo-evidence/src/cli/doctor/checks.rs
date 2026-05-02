//! Per-check implementations for `cargo evidence doctor`. Split
//! out of the parent module to stay under the 500-line workspace
//! file-size limit. Each function returns [`super::CheckResult`]
//! and does NOT emit — so `precheck_doctor` can reuse the pure
//! outcome without generating stdout noise during bundle assembly.

use std::path::{Path, PathBuf};
use std::process::Command;

use evidence_core::FloorsConfig;
use evidence_core::floors::{LoadOutcome, current_measurements, per_crate_measurements};
use evidence_core::policy::{BoundaryConfig, Dal, EvidencePolicy};
use evidence_core::trace::{read_all_trace_files, validate_trace_links_with_policy};

use super::CheckResult;
use crate::cli::trace::default_trace_roots;

pub(super) fn check_trace(workspace: &Path) -> CheckResult {
    // Iterate every configured trace root from the boundary's
    // `scope.trace_roots` or the auto-discovered convention.
    // `default_trace_roots` rebases relative paths against
    // `workspace`.
    let roots = default_trace_roots(workspace);
    let mut sys_reqs = Vec::new();
    let mut hlr_reqs = Vec::new();
    let mut llr_reqs = Vec::new();
    let mut tests = Vec::new();
    let mut derived = Vec::new();
    for root in &roots {
        let files = match read_all_trace_files(root) {
            Ok(f) => f,
            Err(e) => {
                return CheckResult::Fail(
                    "DOCTOR_TRACE_INVALID",
                    format!("could not load trace root {}: {}", root, e),
                );
            }
        };
        sys_reqs.extend(files.sys.requirements);
        hlr_reqs.extend(files.hlr.requirements);
        llr_reqs.extend(files.llr.requirements);
        tests.extend(files.tests.tests);
        if let Some(d) = files.derived {
            derived.extend(d.requirements);
        }
    }

    // DAL drives TracePolicy — hardcoding strict flags would
    // block every real downstream cert build (KNOWN_SURFACES
    // names cargo-evidence's own contracts). DAL-D off; higher
    // levels enable SYS-trace + derived-rationale; surface
    // bijection stays opt-in at every level.
    let (dal, boundary_loadable) = load_max_dal(workspace);
    let policy = EvidencePolicy::for_dal(dal).trace;
    let fallback_note = if boundary_loadable {
        String::new()
    } else {
        " (assumed DAL-D — boundary unloadable; the actual project \
         DAL is unknown, so this check may be looser than the real \
         cert target requires)"
            .to_string()
    };

    // DAL ≥ C gate: a fully-empty trace tree passes
    // `validate_trace_links_with_policy` vacuously — no HLR
    // for DAL-A's `require_hlr_sys_trace` flag to fail on.
    // Fire explicitly so cert-grade targets can't silent-pass
    // on zero data.
    if sys_reqs.is_empty()
        && hlr_reqs.is_empty()
        && llr_reqs.is_empty()
        && tests.is_empty()
        && derived.is_empty()
        && dal >= Dal::C
    {
        return CheckResult::Fail(
            "DOCTOR_TRACE_EMPTY",
            format!(
                "no trace data found at {} for DAL-{:?}{}; cert-grade DAL \
                 requires a populated trace tree.",
                roots.join(", "),
                dal,
                fallback_note
            ),
        );
    }

    match validate_trace_links_with_policy(
        &sys_reqs, &hlr_reqs, &llr_reqs, &tests, &derived, &policy,
    ) {
        Ok(()) => CheckResult::Pass,
        Err(e) => CheckResult::Fail(
            "DOCTOR_TRACE_INVALID",
            format!(
                "trace validation failed at DAL-{:?}{}: {}",
                dal, fallback_note, e
            ),
        ),
    }
}

/// Trace-policy DAL across per-crate overrides. See LLR-060.
/// Returns `(dal, boundary_loadable)`; `false` ⇒ DAL-D fallback.
pub(super) fn load_max_dal(workspace: &Path) -> (Dal, bool) {
    let path = workspace.join("cert").join("boundary.toml");
    let Ok(cfg) = BoundaryConfig::load(&path) else {
        return (Dal::D, false);
    };
    let dal = cfg.dal_map().values().copied().max();
    (dal.unwrap_or(cfg.dal.default_dal), true)
}

pub(super) fn check_floors(workspace: &Path) -> CheckResult {
    let path = workspace.join("cert").join("floors.toml");
    let config = match FloorsConfig::load_or_missing(&path) {
        LoadOutcome::Loaded(c) => c,
        LoadOutcome::Missing => {
            return CheckResult::Fail(
                "DOCTOR_FLOORS_MISSING",
                format!(
                    "no {} — downstream rigor ratchet is not configured. See README \
                     \"`cargo evidence floors` — the ratchet\" for the expected shape.",
                    path.display()
                ),
            );
        }
        LoadOutcome::Error(e) => {
            return CheckResult::Fail(
                "DOCTOR_FLOORS_VIOLATED",
                format!("could not load {}: {}", path.display(), e),
            );
        }
    };
    let measurements = current_measurements(workspace);
    let per_crate = per_crate_measurements(workspace);

    let mut breaches: Vec<String> = Vec::new();
    let mut slack: Vec<String> = Vec::new();
    for (dim, floor) in &config.floors {
        let cur = measurements.get(dim).copied().unwrap_or(0);
        if cur < *floor {
            breaches.push(format!("{} current={} floor={}", dim, cur, floor));
        } else if cur > *floor {
            slack.push(format!("{} current={} floor={}", dim, cur, floor));
        }
    }
    for (crate_name, inner) in &config.per_crate {
        for (dim, floor) in inner {
            let cur = per_crate
                .get(crate_name)
                .and_then(|m| m.get(dim))
                .copied()
                .unwrap_or(0);
            if cur < *floor {
                breaches.push(format!(
                    "{}/{} current={} floor={}",
                    crate_name, dim, cur, floor
                ));
            } else if cur > *floor {
                slack.push(format!(
                    "{}/{} current={} floor={}",
                    crate_name, dim, cur, floor
                ));
            }
        }
    }
    for (crate_name, inner) in &config.per_crate_ceilings {
        for (dim, ceiling) in inner {
            let cur = per_crate
                .get(crate_name)
                .and_then(|m| m.get(dim))
                .copied()
                .unwrap_or(0);
            if cur > *ceiling {
                breaches.push(format!(
                    "{}/{} current={} ceiling={}",
                    crate_name, dim, cur, ceiling
                ));
            }
        }
    }

    // Priority cascade: error-severity findings shadow warning-
    // severity ones. A single CheckResult per check, so pick the
    // highest-severity signal. Order: VIOLATED (error) → BOUNDARY_
    // MISMATCH (warning) → SLACK (warning) → Pass.
    if !breaches.is_empty() {
        return CheckResult::Fail(
            "DOCTOR_FLOORS_VIOLATED",
            format!("floors breached: {}", breaches.join("; ")),
        );
    }
    if let Some(mismatch) = floors_boundary_mismatch(workspace, &config) {
        return CheckResult::Fail("DOCTOR_FLOORS_BOUNDARY_MISMATCH", mismatch);
    }
    if !slack.is_empty() {
        return CheckResult::Fail(
            "DOCTOR_FLOORS_SLACK",
            super::untracked_hint::slack_message_with_hint(workspace, &slack),
        );
    }
    CheckResult::Pass
}

/// Check that `[per_crate.<crate>]` keys in floors.toml match
/// `[scope].in_scope` in boundary.toml. This is the downstream
/// mirror of the internal `per_crate_floors_match_boundary_in_scope`
/// integration test. Returns `None` if the two configs agree or
/// boundary isn't loadable (in which case `check_boundary` already
/// fires its own diagnostic).
fn floors_boundary_mismatch(workspace: &Path, floors: &FloorsConfig) -> Option<String> {
    use std::collections::BTreeSet;
    let boundary_path = workspace.join("cert").join("boundary.toml");
    let boundary = BoundaryConfig::load(&boundary_path).ok()?;
    let in_scope: BTreeSet<&str> = boundary.scope.in_scope.iter().map(String::as_str).collect();
    let declared: BTreeSet<&str> = floors
        .per_crate
        .keys()
        .chain(floors.per_crate_ceilings.keys())
        .map(String::as_str)
        .collect();
    if in_scope == declared {
        return None;
    }
    let missing: Vec<&&str> = in_scope.difference(&declared).collect();
    let extra: Vec<&&str> = declared.difference(&in_scope).collect();
    let mut parts = Vec::new();
    if !missing.is_empty() {
        parts.push(format!(
            "boundary in_scope lists crate(s) with no per_crate floor: {:?}",
            missing
        ));
    }
    if !extra.is_empty() {
        parts.push(format!(
            "floors.toml [per_crate.*] has crate(s) not in boundary in_scope: {:?}",
            extra
        ));
    }
    Some(parts.join("; "))
}

pub(super) fn check_boundary(workspace: &Path) -> CheckResult {
    let path = workspace.join("cert").join("boundary.toml");
    if !path.exists() {
        return CheckResult::Fail(
            "DOCTOR_BOUNDARY_MISSING",
            format!(
                "no {} — the scope boundary between cert-evidence-bearing \
                 crates and sandbox code is not declared. Create the file \
                 with `[scope] in_scope = [...]` at minimum.",
                path.display()
            ),
        );
    }
    match BoundaryConfig::load(&path) {
        Ok(_) => CheckResult::Pass,
        Err(e) => CheckResult::Fail(
            "DOCTOR_BOUNDARY_MISSING",
            format!("could not parse {}: {}", path.display(), e),
        ),
    }
}

pub(super) fn check_ci_integration(workspace: &Path) -> CheckResult {
    let wf_dir = workspace.join(".github").join("workflows");
    if !wf_dir.is_dir() {
        return CheckResult::Fail(
            "DOCTOR_CI_INTEGRATION_MISSING",
            format!(
                "no {}/ — the project has no GitHub Actions workflow calling \
                 cargo evidence. Floors / trace / override-drift gates are \
                 only effective when wired into CI.",
                wf_dir.display()
            ),
        );
    }
    let entries: Vec<PathBuf> = walkdir::WalkDir::new(&wf_dir)
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
        .map(|e| e.into_path())
        .collect();
    for path in &entries {
        if let Ok(text) = std::fs::read_to_string(path)
            && workflow_invokes_cargo_evidence(&text)
        {
            return CheckResult::Pass;
        }
    }
    CheckResult::Fail(
        "DOCTOR_CI_INTEGRATION_MISSING",
        format!(
            "no workflow under {} invokes `cargo evidence` or `cargo-evidence` via a \
             `run:` step. Add a CI step that runs `cargo evidence check` / \
             `doctor` / `floors` so drift gets caught.",
            wf_dir.display()
        ),
    )
}

/// Tighter match than `text.contains("cargo evidence")`: require
/// the invocation to appear within ~200 chars of a `run:` key so
/// prose mentions in workflow comments or README-embedded YAML
/// don't register as "CI integration present."
fn workflow_invokes_cargo_evidence(text: &str) -> bool {
    let needles = ["cargo evidence", "cargo-evidence"];
    // Split on "run:" and inspect the head of each resulting segment
    // (skip the first — it's text BEFORE any `run:` key). Slicing
    // with `split` is UTF-8-safe; a fixed byte window isn't.
    for segment in text.split("run:").skip(1) {
        let window: String = segment.chars().take(200).collect();
        if needles.iter().any(|n| window.contains(n)) {
            return true;
        }
    }
    false
}

pub(super) fn check_merge_style(workspace: &Path) -> CheckResult {
    // The real question: can an `Override-Deterministic-Baseline:` line
    // in a PR body actually reach the push-event gate? Two
    // mitigations suffice:
    //   (a) workflow plumbs `github.event.commits[*].message` as an
    //       additional haystack (survives any merge style)
    //   (b) repo uses squash-merge exclusively (PR body lands in the
    //       squashed head_commit message)
    // If EITHER is in place, no warning. If NEITHER, merge-commit
    // history points at a real gap.

    // (a) Workflow-plumb probe.
    if workflow_plumbs_commits_array(workspace) {
        return CheckResult::Pass;
    }

    // (b) History probe — count merge-commits in recent history
    //     of the default branch. Try `main` first (modern default),
    //     fall back to `master` (older repos, GitHub repos created
    //     before late-2020). If neither resolves, fire UNKNOWN.
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
             request #`), AND no workflow file plumbs \
             `github.event.commits[*].message` as an override haystack. \
             On push-to-main events the gate reads only \
             `head_commit.message` by default, which is the mechanical \
             merge-commit subject — so an Override-Deterministic-\
             Baseline line in the PR body never surfaces. Mitigations: \
             (a) switch to squash-merge in repo Settings → General → \
             Pull Requests; (b) or plumb `github.event.commits[*].message` \
             as a third haystack (see cargo-evidence's own ci.yml).",
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

/// True iff any workflow file under `.github/workflows` references
/// `github.event.commits`, indicating the override-haystack plumbing
/// that neutralizes merge-commit-style risk.
fn workflow_plumbs_commits_array(workspace: &Path) -> bool {
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
                .map(|t| t.contains("github.event.commits"))
                .unwrap_or(false)
        })
}

pub(super) fn check_override_protocol(workspace: &Path) -> CheckResult {
    const NEEDLE: &str = "Override-Deterministic-Baseline:";
    let mut candidates: Vec<PathBuf> = vec![
        workspace.join("README.md"),
        workspace.join("CONTRIBUTING.md"),
    ];
    // Also walk `docs/` (to depth 3) for any `.md` file — real
    // projects often document conventions in `docs/contributing/*`,
    // `docs/cert/*`, etc. Two filenames alone is too narrow.
    let docs_dir = workspace.join("docs");
    if docs_dir.is_dir() {
        candidates.extend(
            walkdir::WalkDir::new(&docs_dir)
                .follow_links(false)
                .max_depth(3)
                .into_iter()
                .filter_map(Result::ok)
                .filter(|e| {
                    e.file_type().is_file()
                        && e.path().extension().and_then(|x| x.to_str()) == Some("md")
                })
                .map(|e| e.into_path()),
        );
    }
    for path in &candidates {
        if let Ok(text) = std::fs::read_to_string(path)
            && text.contains(NEEDLE)
        {
            return CheckResult::Pass;
        }
    }
    CheckResult::Fail(
        "DOCTOR_OVERRIDE_PROTOCOL_UNDOCUMENTED",
        format!(
            "no README.md, CONTRIBUTING.md, or `docs/**/*.md` mentions `{}` — \
             contributors won't know the protocol for intentional \
             reproducibility-input changes. Add a section documenting the \
             override syntax (mechanism, examples, what triggers it).",
            NEEDLE
        ),
    )
}
