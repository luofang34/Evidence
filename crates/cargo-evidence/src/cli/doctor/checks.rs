//! Per-check implementations for `cargo evidence doctor`. Split
//! out of the parent module to stay under the 500-line workspace
//! file-size limit. Each function returns [`super::CheckResult`]
//! and does NOT emit — so `precheck_doctor` can reuse the pure
//! outcome without generating stdout noise during bundle assembly.

use std::path::{Path, PathBuf};

use evidence_core::FloorsConfig;
use evidence_core::floors::{LoadOutcome, current_measurements, per_crate_measurements};
use evidence_core::policy::{AssuranceLevel, BoundaryConfig, Dal, EvidencePolicy};
use evidence_core::trace::{TraceEvidenceState, evaluate_trace_evidence};

use super::CheckResult;
use crate::cli::trace::default_trace_roots;

pub(super) fn check_trace(workspace: &Path) -> CheckResult {
    // One semantic evaluation shared with `trace --validate`,
    // `check`, `generate`, and bundle verify (LLR-105).
    // `default_trace_roots` rebases relative paths against
    // `workspace`.
    let roots = default_trace_roots(workspace);

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

    let eval = evaluate_trace_evidence(&roots, &policy);

    // DAL ≥ C gate: a fully-empty trace tree passes
    // `validate_trace_links_with_policy` vacuously — no HLR
    // for DAL-A's `require_hlr_sys_trace` flag to fail on.
    // Fire explicitly so cert-grade targets can't silent-pass
    // on zero data. The message names the actual adoption state
    // (missing roots vs zero requirements) so the auditor can
    // tell "not adopted yet" apart from "adopted but empty".
    if dal >= Dal::C {
        return match &eval.state {
            TraceEvidenceState::Valid => CheckResult::Pass,
            TraceEvidenceState::Invalid => CheckResult::Fail(
                "DOCTOR_TRACE_INVALID",
                format!(
                    "trace validation failed at DAL-{:?}{}: {}",
                    dal,
                    fallback_note,
                    invalid_detail(&eval)
                ),
            ),
            no_evidence => CheckResult::Fail(
                "DOCTOR_TRACE_EMPTY",
                format!(
                    "no usable trace data found at {} for DAL-{:?}{}; cert-grade DAL \
                     requires a populated trace tree ({})",
                    roots.join(", "),
                    dal,
                    fallback_note,
                    adoption_detail(no_evidence, &eval)
                ),
            ),
        };
    }

    // Development mode (DAL-D): absence of evidence is an adoption
    // state, reported as an explicit warning-severity adoption
    // diagnostic — never a silent pass, never a hard fail.
    match &eval.state {
        TraceEvidenceState::Valid => CheckResult::Pass,
        TraceEvidenceState::Invalid => CheckResult::Fail(
            "DOCTOR_TRACE_INVALID",
            format!(
                "trace validation failed at DAL-{:?}{}: {}",
                dal,
                fallback_note,
                invalid_detail(&eval)
            ),
        ),
        TraceEvidenceState::NotAdopted { missing_roots } => CheckResult::Fail(
            "DOCTOR_TRACE_NOT_ADOPTED",
            format!(
                "trace root(s) configured but missing on disk: {} — trace evidence \
                 is not adopted yet{}",
                missing_roots.join(", "),
                fallback_note
            ),
        ),
        // `Empty` and `NotConfigured` both mean "zero requirements
        // to audit"; when the boundary is unloadable the sibling
        // `check_boundary` check already names the config-side
        // root cause (DOCTOR_BOUNDARY_MISSING).
        TraceEvidenceState::Empty | TraceEvidenceState::NotConfigured => CheckResult::Fail(
            "DOCTOR_TRACE_NO_EVIDENCE",
            format!(
                "trace evidence holds zero requirements at {}{} — an adoption \
                 state, not valid evidence",
                roots.join(", "),
                fallback_note
            ),
        ),
    }
}

/// Render the failure detail for an `Invalid` evaluation: the
/// typed validation error when validation ran, otherwise the
/// read/parse failure that prevented loading.
fn invalid_detail(eval: &evidence_core::trace::TraceEvidenceEval) -> String {
    if let Some(validation) = &eval.validation {
        validation.to_string()
    } else if let Some(read_error) = &eval.read_error {
        format!("could not load trace files: {}", read_error)
    } else {
        "unknown validation failure".to_string()
    }
}

/// Render the adoption-state detail for a no-evidence evaluation
/// under an active claim (DAL ≥ C): distinguishes "roots missing"
/// from "zero requirements" from "no roots configured".
fn adoption_detail(
    state: &TraceEvidenceState,
    eval: &evidence_core::trace::TraceEvidenceEval,
) -> String {
    match state {
        TraceEvidenceState::NotAdopted { .. } => format!(
            "trace root(s) missing on disk: {}",
            eval.missing_roots.join(", ")
        ),
        TraceEvidenceState::Empty => {
            "trace roots present but zero requirements across all layers".to_string()
        }
        TraceEvidenceState::NotConfigured => {
            "no trace roots configured or discoverable".to_string()
        }
        TraceEvidenceState::Invalid | TraceEvidenceState::Valid => String::new(),
    }
}

/// Trace-policy DAL across per-crate overrides. See LLR-060.
/// Returns `(dal, boundary_loadable)`; `false` ⇒ DAL-D fallback.
/// Doctor is an advisory development surface: with no claimed level
/// the least-strict policy row applies (LLR-109), and the
/// load-failure note names the assumption.
pub(super) fn load_max_dal(workspace: &Path) -> (Dal, bool) {
    let path = workspace.join("cert").join("boundary.toml");
    let Ok(cfg) = BoundaryConfig::load(&path) else {
        return (Dal::D, false);
    };
    let dal = cfg
        .dal_map()
        .values()
        .copied()
        .max()
        .map(AssuranceLevel::effective_policy_dal);
    (
        dal.or(cfg.dal.and_then(|d| d.default_dal))
            .unwrap_or(Dal::D),
        true,
    )
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
