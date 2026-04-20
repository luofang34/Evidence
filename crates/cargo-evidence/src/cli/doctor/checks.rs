//! Per-check implementations for `cargo evidence doctor`. Split
//! out of the parent module to stay under the 500-line workspace
//! file-size limit. Each function returns [`super::CheckResult`]
//! and does NOT emit — so `precheck_doctor` can reuse the pure
//! outcome without generating stdout noise during bundle assembly.

use std::path::{Path, PathBuf};
use std::process::Command;

use evidence::FloorsConfig;
use evidence::floors::{LoadOutcome, current_measurements, per_crate_measurements};
use evidence::policy::{BoundaryConfig, TracePolicy};
use evidence::trace::{read_all_trace_files, validate_trace_links_with_policy};

use super::CheckResult;

pub(super) fn check_trace(workspace: &Path) -> CheckResult {
    let trace_root = workspace.join("tool").join("trace");
    let trace_root_str = match trace_root.to_str() {
        Some(s) => s,
        None => {
            return CheckResult::Fail(
                "DOCTOR_TRACE_INVALID",
                format!("tool/trace path is not UTF-8: {}", trace_root.display()),
            );
        }
    };
    let files = match read_all_trace_files(trace_root_str) {
        Ok(f) => f,
        Err(e) => {
            return CheckResult::Fail(
                "DOCTOR_TRACE_INVALID",
                format!(
                    "could not load tool/trace/ at {}: {}",
                    trace_root.display(),
                    e
                ),
            );
        }
    };
    let policy = TracePolicy {
        require_hlr_sys_trace: true,
        require_hlr_surface_bijection: true,
        require_derived_rationale: true,
        ..TracePolicy::default()
    };
    let sys_reqs = files.sys.requirements.clone();
    let hlr_reqs = files.hlr.requirements.clone();
    let llr_reqs = files.llr.requirements.clone();
    let tests = files.tests.tests.clone();
    let derived = files
        .derived
        .as_ref()
        .map(|d| d.requirements.clone())
        .unwrap_or_default();
    match validate_trace_links_with_policy(
        &sys_reqs, &hlr_reqs, &llr_reqs, &tests, &derived, &policy,
    ) {
        Ok(()) => CheckResult::Pass,
        Err(e) => CheckResult::Fail(
            "DOCTOR_TRACE_INVALID",
            format!("tool/trace/ validation failed: {}", e),
        ),
    }
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
    for (dim, floor) in &config.floors {
        let cur = measurements.get(dim).copied().unwrap_or(0);
        if cur < *floor {
            breaches.push(format!("{} current={} floor={}", dim, cur, floor));
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

    if breaches.is_empty() {
        CheckResult::Pass
    } else {
        CheckResult::Fail(
            "DOCTOR_FLOORS_VIOLATED",
            format!("floors breached: {}", breaches.join("; ")),
        )
    }
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
            && (text.contains("cargo evidence") || text.contains("cargo-evidence"))
        {
            return CheckResult::Pass;
        }
    }
    CheckResult::Fail(
        "DOCTOR_CI_INTEGRATION_MISSING",
        format!(
            "no workflow under {} mentions `cargo evidence` or `cargo-evidence`. \
             Add a CI step that runs `cargo evidence check` / `doctor` / `floors` \
             so drift gets caught.",
            wf_dir.display()
        ),
    )
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

    // (b) History probe — count merge-commits in recent main history.
    let out = Command::new("git")
        .args(["log", "-n", "20", "--format=%s", "main"])
        .current_dir(workspace)
        .output();
    let stdout = match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).into_owned(),
        Ok(o) => {
            return CheckResult::Fail(
                "DOCTOR_MERGE_STYLE_UNKNOWN",
                format!(
                    "git log on main branch failed (exit {:?}); merge-style policy \
                     could not be audited. Run `git log -n 20 main` manually to \
                     diagnose, then re-run doctor.",
                    o.status.code()
                ),
            );
        }
        Err(e) => {
            return CheckResult::Fail(
                "DOCTOR_MERGE_STYLE_UNKNOWN",
                format!(
                    "git unavailable ({}); merge-style policy could not be audited. \
                     Install git or re-run doctor in a repo clone.",
                    e
                ),
            );
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
    let candidates = [
        workspace.join("README.md"),
        workspace.join("CONTRIBUTING.md"),
    ];
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
            "no README.md or CONTRIBUTING.md mentions `{}` — contributors \
             won't know the protocol for intentional reproducibility-input \
             changes. Add a section documenting the override syntax \
             (mechanism, examples, what triggers it).",
            NEEDLE
        ),
    )
}
