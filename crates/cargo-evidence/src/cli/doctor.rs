//! `cargo evidence doctor` — downstream rigor audit (LLR-048).
//!
//! Runs a fixed-order checklist against the current workspace and
//! emits one `Diagnostic` per check plus one terminal
//! (`DOCTOR_OK` / `DOCTOR_FAIL`) per Schema Rule 1. Jsonl-native.
//!
//! The six MVP checks:
//!
//! - **trace validity** (`DOCTOR_TRACE_INVALID` on fail) — load
//!   `tool/trace/` and run the SYS / surface / selector bijections
//!   the tool's own CI enables on itself.
//! - **floors present + satisfied**
//!   (`DOCTOR_FLOORS_MISSING` / `DOCTOR_FLOORS_VIOLATED`) — load
//!   `cert/floors.toml`, compute measurements, fail if any floor is
//!   breached.
//! - **boundary config** (`DOCTOR_BOUNDARY_MISSING`) — load
//!   `cert/boundary.toml`.
//! - **CI integration** (`DOCTOR_CI_INTEGRATION_MISSING` warning) —
//!   grep `.github/workflows/*.yml` for `cargo evidence`.
//! - **merge-style policy** (`DOCTOR_MERGE_STYLE_RISK` warning) —
//!   scan last 20 main-branch commits for `Merge pull request #N`
//!   pattern; warn because merge-commit style leaves the
//!   `Override-Deterministic-Baseline:` line in the PR body
//!   invisible to the push-event gate.
//! - **override protocol docs**
//!   (`DOCTOR_OVERRIDE_PROTOCOL_UNDOCUMENTED` warning) — grep
//!   `README.md` + `CONTRIBUTING.md` for the override string so
//!   contributors know the convention.
//!
//! Every check that passes emits `DOCTOR_CHECK_PASSED` with the
//! check name in `message`, so the stream shape is exactly one
//! line per check plus the terminal. Downstream tooling can
//! detect truncation.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Result;

use evidence::FloorsConfig;
use evidence::diagnostic::{Diagnostic, Severity};
use evidence::floors::{LoadOutcome, current_measurements, per_crate_measurements};
use evidence::policy::{BoundaryConfig, TracePolicy};
use evidence::trace::{read_all_trace_files, validate_trace_links_with_policy};

use super::args::{EXIT_SUCCESS, EXIT_VERIFICATION_FAILURE};
use super::output::emit_jsonl;

const SUBCOMMAND: &str = "doctor";

/// Entrypoint for `cargo evidence doctor`.
///
/// Runs each check in a fixed deterministic order, streams one
/// diagnostic per check, and terminates with `DOCTOR_OK` (exit 0)
/// or `DOCTOR_FAIL` (exit 2) depending on whether any error-
/// severity finding fired.
pub fn cmd_doctor() -> Result<i32> {
    let workspace = std::env::current_dir()?;

    let mut any_error = false;
    any_error |= run_check("trace validity", check_trace(&workspace))?;
    any_error |= run_check("floors config", check_floors(&workspace))?;
    any_error |= run_check("boundary config", check_boundary(&workspace))?;
    // Warning-severity checks never flip any_error — they don't
    // block cert-profile generation, but they appear in the
    // stream so auditors see the gap.
    let _ = run_check("CI integration", check_ci_integration(&workspace))?;
    let _ = run_check("merge-style policy", check_merge_style(&workspace))?;
    let _ = run_check(
        "override protocol docs",
        check_override_protocol(&workspace),
    )?;

    let terminal = if any_error {
        terminal_fail()
    } else {
        terminal_ok()
    };
    emit_jsonl(&terminal)?;

    Ok(if any_error {
        EXIT_VERIFICATION_FAILURE
    } else {
        EXIT_SUCCESS
    })
}

/// Public entrypoint for `generate --profile cert` / `record` to
/// gate bundle assembly on a successful audit. Returns
/// `Err(anyhow!(...))` when any error-severity check fires; the
/// error's `.to_string()` enumerates the triggered DOCTOR_* codes
/// so the caller can surface them in a `GENERATE_ERROR` terminal
/// without rerunning doctor.
pub fn precheck_doctor(workspace: &Path) -> Result<()> {
    let mut failed_codes: Vec<&'static str> = Vec::new();

    for (_, result) in [
        ("trace validity", check_trace(workspace)),
        ("floors config", check_floors(workspace)),
        ("boundary config", check_boundary(workspace)),
    ] {
        if let CheckResult::Fail(code, _) = result {
            failed_codes.push(code);
        }
    }

    if failed_codes.is_empty() {
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "cargo evidence doctor precheck failed: {} — run `cargo evidence doctor` \
             for details",
            failed_codes.join(", ")
        ))
    }
}

/// Outcome of a single check. The check function doesn't emit
/// directly so `precheck_doctor` can reuse it without generating
/// stdout noise during bundle assembly.
enum CheckResult {
    Pass,
    Fail(&'static str, String),
}

fn run_check(name: &str, result: CheckResult) -> Result<bool> {
    match result {
        CheckResult::Pass => {
            emit_jsonl(&check_passed(name))?;
            Ok(false)
        }
        CheckResult::Fail(code, message) => {
            let severity = if is_warning_code(code) {
                Severity::Warning
            } else {
                Severity::Error
            };
            emit_jsonl(&Diagnostic {
                code: code.to_string(),
                severity,
                message,
                location: None,
                fix_hint: None,
                subcommand: Some(SUBCOMMAND.to_string()),
                root_cause_uid: None,
            })?;
            Ok(severity == Severity::Error)
        }
    }
}

fn is_warning_code(code: &str) -> bool {
    matches!(
        code,
        "DOCTOR_CI_INTEGRATION_MISSING"
            | "DOCTOR_MERGE_STYLE_RISK"
            | "DOCTOR_OVERRIDE_PROTOCOL_UNDOCUMENTED"
    )
}

fn check_passed(name: &str) -> Diagnostic {
    Diagnostic {
        code: "DOCTOR_CHECK_PASSED".to_string(),
        severity: Severity::Info,
        message: format!("{}: ok", name),
        location: None,
        fix_hint: None,
        subcommand: Some(SUBCOMMAND.to_string()),
        root_cause_uid: None,
    }
}

fn terminal_ok() -> Diagnostic {
    Diagnostic {
        code: "DOCTOR_OK".to_string(),
        severity: Severity::Info,
        message: "doctor: all error-severity checks passed".to_string(),
        location: None,
        fix_hint: None,
        subcommand: Some(SUBCOMMAND.to_string()),
        root_cause_uid: None,
    }
}

fn terminal_fail() -> Diagnostic {
    Diagnostic {
        code: "DOCTOR_FAIL".to_string(),
        severity: Severity::Error,
        message: "doctor: at least one error-severity check fired".to_string(),
        location: None,
        fix_hint: None,
        subcommand: Some(SUBCOMMAND.to_string()),
        root_cause_uid: None,
    }
}

// ---------------------------------------------------------------------------
// Per-check implementations. Each returns `CheckResult` and does NOT emit
// (so `precheck_doctor` can reuse the pure outcome without stdout noise).
// ---------------------------------------------------------------------------

fn check_trace(workspace: &Path) -> CheckResult {
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

fn check_floors(workspace: &Path) -> CheckResult {
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

fn check_boundary(workspace: &Path) -> CheckResult {
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

fn check_ci_integration(workspace: &Path) -> CheckResult {
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

fn check_merge_style(workspace: &Path) -> CheckResult {
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
        _ => return CheckResult::Pass, // git unavailable — don't penalize.
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

fn check_override_protocol(workspace: &Path) -> CheckResult {
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
