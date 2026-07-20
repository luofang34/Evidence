//! Compose a [`ContextReport`] from the trace graph, boundary
//! policy, floors config, and the layered `CLAUDE.md` set.
//!
//! The lookup pass is deliberately read-only: it ingests data via
//! the existing `read_all_trace_files`, `BoundaryConfig::load`, and
//! `FloorsConfig::load_or_missing` helpers and never touches disk
//! beyond those calls + the `CLAUDE.md` existence probe.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use super::error::ContextError;
use super::report::{
    BoundarySlice, ContextReport, ContextWarning, Conventions, FloorRow, ParentRef, RequirementRef,
    SelectorView, TestRef,
};
use super::resolver::{ResolvedSelector, discover_workspace_crates};
use crate::floors::{FloorsConfig, LoadOutcome, current_measurements, per_crate_measurements};
use crate::policy::{AssuranceLevel, BoundaryConfig};
use crate::trace::{HlrEntry, LlrEntry, TestEntry, TraceFiles, read_all_trace_files};

/// Top-level entry point — compose the report for `selector`.
pub fn context_for(
    workspace_root: &Path,
    selector: &ResolvedSelector,
) -> Result<ContextReport, ContextError> {
    let trace_root = workspace_root.join("cert").join("trace");
    if !trace_root.is_dir() {
        return Err(ContextError::TraceNotConfigured(trace_root));
    }
    let trace = read_all_trace_files(trace_root.to_string_lossy().as_ref())?;
    let boundary = BoundaryConfig::load_or_default(&workspace_root.join("cert/boundary.toml"));
    let floors_cfg = match FloorsConfig::load_or_missing(&workspace_root.join("cert/floors.toml")) {
        LoadOutcome::Loaded(cfg) => Some(cfg),
        LoadOutcome::Missing | LoadOutcome::Error(_) => None,
    };

    let report = match selector {
        ResolvedSelector::Workspace => {
            build_workspace_overview(workspace_root, &boundary, floors_cfg.as_ref())
        }
        _ => build_scoped_report(
            workspace_root,
            selector,
            &trace,
            &boundary,
            floors_cfg.as_ref(),
        )?,
    };
    Ok(report)
}

/// Workspace-overview path — empty selector returns the high-level
/// crate map + workspace-wide floors + the root `CLAUDE.md` pointer.
fn build_workspace_overview(
    workspace_root: &Path,
    boundary: &BoundaryConfig,
    floors: Option<&FloorsConfig>,
) -> ContextReport {
    let mut report = ContextReport::workspace_default();
    report.boundary = BoundarySlice {
        in_scope: true,
        forbidden_deps: boundary.scope.explicit_forbidden.clone(),
    };
    if let Some(cfg) = floors {
        let measured = current_measurements(workspace_root);
        let mut rows: Vec<FloorRow> = cfg
            .floors
            .iter()
            .map(|(dim, &floor)| FloorRow {
                dimension: dim.clone(),
                kind: "floor".to_string(),
                current: measured.get(dim).copied().unwrap_or(0),
                floor,
            })
            .collect();
        rows.sort_by(|a, b| a.dimension.cmp(&b.dimension));
        report.floors = rows;
    }
    report.conventions = Conventions {
        nearest_claude_md: workspace_claude_md(workspace_root),
    };
    report
}

/// Per-selector report. Re-uses the trace graph for requirement /
/// parent / test rollup, the boundary `dal_map` for the effective
/// DAL, and the per-crate floors slice for the floors block.
fn build_scoped_report(
    workspace_root: &Path,
    selector: &ResolvedSelector,
    trace: &TraceFiles,
    boundary: &BoundaryConfig,
    floors: Option<&FloorsConfig>,
) -> Result<ContextReport, ContextError> {
    let (crate_name, selector_path, selector_input) = describe_selector(selector);
    let llrs = filter_llrs_for_selector(selector, &trace.llr.requirements);
    let parents = roll_up_parents(&llrs, &trace.hlr.requirements, &trace.sys.requirements);
    let tests = filter_tests_for_llrs(&llrs, &trace.tests.tests);
    let diagnostic_codes = collect_emits(&llrs);

    let dal = dal_for_crate(boundary, crate_name.as_deref());
    let floors_rows = floors
        .map(|cfg| per_crate_floor_rows(workspace_root, cfg, crate_name.as_deref()))
        .unwrap_or_default();

    let in_scope = crate_name
        .as_ref()
        .map(|name| boundary.scope.in_scope.iter().any(|c| c == name))
        .unwrap_or(false);

    let conventions = Conventions {
        nearest_claude_md: nearest_claude_md(workspace_root, crate_name.as_deref()),
    };

    let warnings = collect_warnings(selector, &llrs);

    Ok(ContextReport {
        selector: SelectorView {
            kind: selector.kind().to_string(),
            input: selector_input,
            resolved: selector_path,
        },
        crate_name: crate_name.clone().unwrap_or_default(),
        dal,
        requirements: llrs.iter().map(llr_to_ref).collect(),
        parents,
        tests,
        diagnostic_codes,
        floors: floors_rows,
        boundary: BoundarySlice {
            in_scope,
            forbidden_deps: boundary.scope.explicit_forbidden.clone(),
        },
        conventions,
        warnings,
    })
}

fn describe_selector(selector: &ResolvedSelector) -> (Option<String>, String, String) {
    match selector {
        ResolvedSelector::Workspace => (None, String::new(), String::new()),
        ResolvedSelector::File {
            raw,
            path,
            crate_name,
            ..
        } => (Some(crate_name.clone()), path.clone(), raw.clone()),
        ResolvedSelector::Crate {
            raw, crate_name, ..
        } => (Some(crate_name.clone()), crate_name.clone(), raw.clone()),
        ResolvedSelector::Module { raw, path, .. } => {
            let crate_name = path
                .split("::")
                .next()
                .map(|s| s.replace('_', "-"))
                .filter(|s| !s.is_empty());
            (crate_name, path.clone(), raw.clone())
        }
    }
}

/// Filter LLRs whose `modules` field overlaps the selector's
/// module space. For file / crate / module selectors we match against
/// a derived module-prefix; the workspace path uses every LLR.
fn filter_llrs_for_selector(selector: &ResolvedSelector, llrs: &[LlrEntry]) -> Vec<LlrEntry> {
    let prefixes = selector_prefixes(selector);
    if prefixes.is_empty() {
        return llrs.to_vec();
    }
    llrs.iter()
        .filter(|llr| llr.modules.iter().any(|m| matches_any_prefix(m, &prefixes)))
        .cloned()
        .collect()
}

/// Compute the set of acceptable `modules`-field prefixes for the
/// selector. File and module selectors share the same prefix; crate
/// selectors match by `<crate_root>::*` (and the bare `<crate_root>`).
fn selector_prefixes(selector: &ResolvedSelector) -> Vec<String> {
    match selector {
        ResolvedSelector::Workspace => Vec::new(),
        ResolvedSelector::File { crate_name, .. } | ResolvedSelector::Crate { crate_name, .. } => {
            vec![crate_name.replace('-', "_")]
        }
        ResolvedSelector::Module { path, .. } => vec![path.clone()],
    }
}

/// `module` matches `prefix` iff `module == prefix` or
/// `module` starts with `prefix::`. We also accept the
/// reverse (e.g. selector "evidence_core::trace" matches LLR module
/// "evidence_core::trace::validation") since the spec asks for
/// prefix-match on either side.
fn matches_any_prefix(module: &str, prefixes: &[String]) -> bool {
    prefixes.iter().any(|p| {
        module == p
            || module.starts_with(&format!("{}::", p))
            || p.starts_with(&format!("{}::", module))
    })
}

/// Collect the HLR / SYS rows reached from each LLR's `traces_to`.
/// HLRs go one level up to SYS; the result is a flat list sorted by
/// `id` for deterministic serialization.
fn roll_up_parents(
    llrs: &[LlrEntry],
    hlr_pool: &[HlrEntry],
    sys_pool: &[HlrEntry],
) -> Vec<ParentRef> {
    let hlr_by_uid: BTreeMap<&str, &HlrEntry> = hlr_pool
        .iter()
        .filter_map(|h| h.uid.as_deref().map(|u| (u, h)))
        .collect();
    let sys_by_uid: BTreeMap<&str, &HlrEntry> = sys_pool
        .iter()
        .filter_map(|s| s.uid.as_deref().map(|u| (u, s)))
        .collect();

    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut rows: Vec<ParentRef> = Vec::new();

    for llr in llrs {
        for parent_uid in &llr.traces_to {
            if !seen.insert(parent_uid.clone()) {
                continue;
            }
            if let Some(hlr) = hlr_by_uid.get(parent_uid.as_str()) {
                rows.push(hlr_to_parent_ref(hlr));
                for grandparent_uid in &hlr.traces_to {
                    if !seen.insert(grandparent_uid.clone()) {
                        continue;
                    }
                    if let Some(sys) = sys_by_uid.get(grandparent_uid.as_str()) {
                        rows.push(sys_to_parent_ref(sys));
                    }
                }
            }
        }
    }
    rows.sort_by(|a, b| a.id.cmp(&b.id));
    rows
}

/// Filter test entries that trace into any of the listed LLRs (by
/// uid) and sort by `name`.
fn filter_tests_for_llrs(llrs: &[LlrEntry], tests: &[TestEntry]) -> Vec<TestRef> {
    let llr_uids: BTreeSet<&str> = llrs.iter().filter_map(|l| l.uid.as_deref()).collect();
    let mut rows: Vec<TestRef> = tests
        .iter()
        .filter(|t| {
            t.traces_to
                .iter()
                .any(|uid| llr_uids.contains(uid.as_str()))
        })
        .map(test_to_ref)
        .collect();
    rows.sort_by(|a, b| a.name.cmp(&b.name));
    rows
}

/// Collect every diagnostic code claimed by the listed LLRs'
/// `emits`. Deduped + alphabetically sorted.
fn collect_emits(llrs: &[LlrEntry]) -> Vec<String> {
    let mut set: BTreeSet<String> = BTreeSet::new();
    for llr in llrs {
        for code in &llr.emits {
            set.insert(code.clone());
        }
    }
    set.into_iter().collect()
}

/// Resolve the effective assurance-level label for `crate_name`:
/// the crate's override, else `default_dal`, else `unclassified`
/// when no level is claimed (LLR-109).
fn dal_for_crate(boundary: &BoundaryConfig, crate_name: Option<&str>) -> String {
    let claimed = boundary.dal.as_ref().and_then(|dal| {
        crate_name
            .and_then(|name| dal.crate_overrides.get(name).copied())
            .or(dal.default_dal)
    });
    claimed
        .map_or(AssuranceLevel::Unclassified, AssuranceLevel::from_dal)
        .to_string()
}

/// Per-crate floor rows applicable to the selector. Workspace-wide
/// floors (`[floors]`) are omitted from per-crate reports — the
/// workspace overview owns those.
fn per_crate_floor_rows(
    workspace_root: &Path,
    cfg: &FloorsConfig,
    crate_name: Option<&str>,
) -> Vec<FloorRow> {
    let Some(crate_name) = crate_name else {
        return Vec::new();
    };
    let measured = per_crate_measurements(workspace_root);
    let per_crate_floors = cfg.per_crate.get(crate_name);
    let per_crate_ceilings = cfg.per_crate_ceilings.get(crate_name);
    let measured_for_crate = measured.get(crate_name);

    let mut rows: Vec<FloorRow> = Vec::new();
    if let Some(map) = per_crate_floors {
        for (dim, &floor) in map {
            let current = measured_for_crate
                .and_then(|m| m.get(dim).copied())
                .unwrap_or(0);
            rows.push(FloorRow {
                dimension: dim.clone(),
                kind: "per_crate_floor".to_string(),
                current,
                floor,
            });
        }
    }
    if let Some(map) = per_crate_ceilings {
        for (dim, &ceiling) in map {
            let current = measured_for_crate
                .and_then(|m| m.get(dim).copied())
                .unwrap_or(0);
            rows.push(FloorRow {
                dimension: dim.clone(),
                kind: "per_crate_ceiling".to_string(),
                current,
                floor: ceiling,
            });
        }
    }
    rows.sort_by(|a, b| a.dimension.cmp(&b.dimension).then(a.kind.cmp(&b.kind)));
    rows
}

/// Find the nearest `CLAUDE.md`: per-crate when `crate_name` is set
/// and reachable, the workspace root otherwise, or `None` if neither
/// exists.
fn nearest_claude_md(workspace_root: &Path, crate_name: Option<&str>) -> Option<String> {
    if let Some(name) = crate_name {
        if let Ok(crates) = discover_workspace_crates(workspace_root) {
            if let Some(entry) = crates.get(name) {
                let path = format!("{}/CLAUDE.md", entry.dir);
                if workspace_root.join(&path).is_file() {
                    return Some(path);
                }
            }
        }
    }
    workspace_claude_md(workspace_root)
}

/// Convenience wrapper for the root-level `CLAUDE.md` lookup.
fn workspace_claude_md(workspace_root: &Path) -> Option<String> {
    let root = workspace_root.join("CLAUDE.md");
    if root.is_file() {
        Some("CLAUDE.md".to_string())
    } else {
        None
    }
}

fn collect_warnings(selector: &ResolvedSelector, llrs: &[LlrEntry]) -> Vec<ContextWarning> {
    let mut out: Vec<ContextWarning> = Vec::new();
    let amb = selector.ambiguities();
    if !amb.is_empty() {
        out.push(ContextWarning {
            code: "CONTEXT_AMBIGUOUS_SELECTOR".to_string(),
            message: format!(
                "selector also matched: {} — resolver picked '{}'",
                amb.join(", "),
                selector.kind()
            ),
        });
    }
    if llrs.is_empty() && !matches!(selector, ResolvedSelector::Workspace) {
        out.push(ContextWarning {
            code: "CONTEXT_NO_REQUIREMENTS_FOR_SELECTOR".to_string(),
            message: "selector matched no LLR — module not yet covered by trace".to_string(),
        });
    }
    out
}

fn llr_to_ref(llr: &LlrEntry) -> RequirementRef {
    RequirementRef {
        id: llr.id.clone(),
        uid: llr.uid.clone().unwrap_or_default(),
        layer: "llr".to_string(),
        title: llr.title.clone(),
        description: llr.description.clone().unwrap_or_default(),
        modules: llr.modules.clone(),
        emits: llr.emits.clone(),
        traces_to: llr.traces_to.clone(),
        verification_methods: llr.verification_methods.clone(),
    }
}

fn hlr_to_parent_ref(hlr: &HlrEntry) -> ParentRef {
    ParentRef {
        id: hlr.id.clone(),
        uid: hlr.uid.clone().unwrap_or_default(),
        layer: "hlr".to_string(),
        title: hlr.title.clone(),
        traces_to: hlr.traces_to.clone(),
    }
}

fn sys_to_parent_ref(sys: &HlrEntry) -> ParentRef {
    ParentRef {
        id: sys.id.clone(),
        uid: sys.uid.clone().unwrap_or_default(),
        layer: "sys".to_string(),
        title: sys.title.clone(),
        traces_to: sys.traces_to.clone(),
    }
}

fn test_to_ref(t: &TestEntry) -> TestRef {
    TestRef {
        id: t.id.clone(),
        uid: t.uid.clone().unwrap_or_default(),
        name: t.title.clone(),
        selectors: t.all_selectors(),
        traces_to: t.traces_to.clone(),
    }
}
