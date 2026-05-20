//! Unit tests for the [`context`](super) module.
//!
//! Three slices, each `#[test]`-fn-scoped so a regression names the
//! exact case that broke: selector classification, lookup composition,
//! and error variants. The repo's own `cert/trace/` is the canonical
//! fixture — building against a synthetic mini-workspace would only
//! prove the test plumbing parses TOML.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use std::path::PathBuf;

use super::error::ContextError;
use super::resolver::{ResolvedSelector, resolve_selector};
use super::{ContextReport, context_for};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

// ============================================================================
// resolver tests (selector classification)
// ============================================================================

/// `raw = None` short-circuits to `Workspace` without touching the
/// filesystem.
#[test]
fn resolve_none_returns_workspace() {
    let resolved = resolve_selector(&workspace_root(), None).expect("resolve");
    assert!(matches!(resolved, ResolvedSelector::Workspace));
}

/// An empty / whitespace-only selector is the same as `None` — treat
/// as workspace overview rather than a typo error.
#[test]
fn resolve_empty_string_returns_workspace() {
    let resolved = resolve_selector(&workspace_root(), Some("")).expect("resolve");
    assert!(matches!(resolved, ResolvedSelector::Workspace));
    let resolved_ws = resolve_selector(&workspace_root(), Some("   ")).expect("resolve");
    assert!(matches!(resolved_ws, ResolvedSelector::Workspace));
}

/// A workspace-relative file under `crates/evidence-core/src/...`
/// classifies as `File` and carries the crate name.
#[test]
fn resolve_file_under_crates_dir() {
    let resolved = resolve_selector(&workspace_root(), Some("crates/evidence-core/src/trace.rs"))
        .expect("resolve");
    match resolved {
        ResolvedSelector::File {
            path, crate_name, ..
        } => {
            assert_eq!(path, "crates/evidence-core/src/trace.rs");
            assert_eq!(crate_name, "evidence-core");
        }
        other => panic!("expected File, got {:?}", other),
    }
}

/// A bare workspace crate name resolves as `Crate`.
#[test]
fn resolve_crate_by_package_name() {
    let resolved = resolve_selector(&workspace_root(), Some("evidence-core")).expect("resolve");
    match resolved {
        ResolvedSelector::Crate { crate_name, .. } => {
            assert_eq!(crate_name, "evidence-core");
        }
        other => panic!("expected Crate, got {:?}", other),
    }
}

/// A `::`-shaped string with no matching file resolves as `Module`.
#[test]
fn resolve_module_by_dotted_path() {
    let resolved =
        resolve_selector(&workspace_root(), Some("evidence_core::trace")).expect("resolve");
    match resolved {
        ResolvedSelector::Module { path, .. } => {
            assert_eq!(path, "evidence_core::trace");
        }
        other => panic!("expected Module, got {:?}", other),
    }
}

/// A typo / unrecognized selector surfaces
/// `SelectorOutOfScope` — caller distinguishes from the legitimate
/// `Workspace` shortcut.
#[test]
fn resolve_unknown_returns_out_of_scope() {
    let err = resolve_selector(&workspace_root(), Some("not-a-crate"))
        .expect_err("must reject unknown selectors");
    match err {
        ContextError::SelectorOutOfScope(input) => assert_eq!(input, "not-a-crate"),
        other => panic!("expected SelectorOutOfScope, got {:?}", other),
    }
}

// ============================================================================
// lookup tests (context composition)
// ============================================================================

/// Workspace overview returns the global crate slice + the root
/// `CLAUDE.md` pointer.
#[test]
fn workspace_overview_carries_root_claude_md() {
    let report = context_for(&workspace_root(), &ResolvedSelector::Workspace).expect("ctx");
    assert_eq!(report.selector.kind, "workspace");
    assert_eq!(report.crate_name, "");
    assert!(report.conventions.nearest_claude_md.is_some());
    assert_eq!(
        report.conventions.nearest_claude_md.as_deref(),
        Some("CLAUDE.md")
    );
}

/// File-selector report carries the per-crate `CLAUDE.md` pointer.
#[test]
fn file_selector_carries_per_crate_claude_md() {
    let selector = resolve_selector(&workspace_root(), Some("crates/evidence-core/src/trace.rs"))
        .expect("resolve");
    let report = context_for(&workspace_root(), &selector).expect("ctx");
    assert_eq!(report.selector.kind, "file");
    assert_eq!(report.crate_name, "evidence-core");
    assert_eq!(
        report.conventions.nearest_claude_md.as_deref(),
        Some("crates/evidence-core/CLAUDE.md")
    );
}

/// File selector under an LLR-claimed module returns at least one
/// requirement and its rolled-up parents — guards against the
/// trace-empty regression that would silently strip the report.
#[test]
fn file_selector_pulls_requirements_and_parents() {
    let selector = resolve_selector(
        &workspace_root(),
        Some("crates/cargo-evidence/src/cli/rules.rs"),
    )
    .expect("resolve");
    let report = context_for(&workspace_root(), &selector).expect("ctx");
    assert!(
        !report.requirements.is_empty(),
        "expected at least one LLR for cargo-evidence's cli/rules.rs"
    );
    let req_layers: Vec<&str> = report
        .requirements
        .iter()
        .map(|r| r.layer.as_str())
        .collect();
    for layer in &req_layers {
        assert_eq!(*layer, "llr", "every requirement row must be an LLR");
    }
    // Parents are rolled-up — at least one HLR or SYS should be
    // present for any non-empty requirements list.
    assert!(
        !report.parents.is_empty(),
        "non-empty requirements must produce at least one parent row"
    );
}

/// Crate selector populates per-crate floor rows.
#[test]
fn crate_selector_carries_per_crate_floor_rows() {
    let selector = resolve_selector(&workspace_root(), Some("evidence-core")).expect("resolve");
    let report = context_for(&workspace_root(), &selector).expect("ctx");
    assert_eq!(report.crate_name, "evidence-core");
    let has_test_count = report
        .floors
        .iter()
        .any(|f| f.dimension == "test_count" && f.kind == "per_crate_floor");
    assert!(
        has_test_count,
        "evidence-core should carry a per_crate_floor row for test_count, got {:?}",
        report.floors
    );
    let has_panics_ceiling = report
        .floors
        .iter()
        .any(|f| f.dimension == "library_panics" && f.kind == "per_crate_ceiling");
    assert!(
        has_panics_ceiling,
        "evidence-core should carry a per_crate_ceiling row for library_panics, got {:?}",
        report.floors
    );
}

/// Diagnostic codes block aggregates every `emits` from matched LLRs.
#[test]
fn requirements_emit_set_aggregates_diagnostic_codes() {
    let selector = resolve_selector(&workspace_root(), Some("cargo-evidence")).expect("resolve");
    let report = context_for(&workspace_root(), &selector).expect("ctx");
    // Empty cargo-evidence diagnostic_codes would mean the per-crate
    // selector matched nothing — the cargo-evidence crate owns many
    // CLI emits.
    assert!(
        !report.diagnostic_codes.is_empty(),
        "cargo-evidence selector should aggregate at least one emit"
    );
    // Codes are sorted alphabetically; the first must be <= the last.
    let codes = &report.diagnostic_codes;
    let mut sorted = codes.clone();
    sorted.sort();
    assert_eq!(
        codes, &sorted,
        "diagnostic_codes must be alphabetically sorted"
    );
}

/// Boundary slice reports `in_scope = true` for the workspace's own
/// crates (every member is in `boundary.toml`).
#[test]
fn boundary_slice_reports_in_scope_for_workspace_crate() {
    let selector = resolve_selector(&workspace_root(), Some("evidence-core")).expect("resolve");
    let report = context_for(&workspace_root(), &selector).expect("ctx");
    assert!(report.boundary.in_scope, "evidence-core must be in scope");
}

/// Round-trip serialize / deserialize keeps the wire shape stable.
#[test]
fn context_report_round_trips_via_serde_json() {
    let selector = resolve_selector(&workspace_root(), Some("evidence-core")).expect("resolve");
    let report = context_for(&workspace_root(), &selector).expect("ctx");
    let s = serde_json::to_string(&report).expect("serialize");
    let back: ContextReport = serde_json::from_str(&s).expect("deserialize");
    assert_eq!(report, back);
}

// ============================================================================
// error tests
// ============================================================================

/// Workspace without `cert/trace/` returns `TraceNotConfigured`
/// rather than blowing up — that's the non-adopter graceful path
/// the CLI converts to `CONTEXT_NO_TRACE_CONFIGURED` (exit 0).
#[test]
fn missing_trace_root_returns_trace_not_configured() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let err = context_for(tmp.path(), &ResolvedSelector::Workspace)
        .expect_err("missing cert/trace must error");
    match err {
        ContextError::TraceNotConfigured(path) => {
            assert!(
                path.ends_with("cert/trace"),
                "TraceNotConfigured must carry cert/trace path, got {:?}",
                path
            );
        }
        other => panic!("expected TraceNotConfigured, got {:?}", other),
    }
}

/// `SelectorOutOfScope` carries the raw input verbatim so the
/// caller can echo it back in a fix-hint.
#[test]
fn selector_out_of_scope_preserves_raw_input() {
    let err =
        resolve_selector(&workspace_root(), Some("does/not/exist.rs")).expect_err("must reject");
    match err {
        ContextError::SelectorOutOfScope(input) => assert_eq!(input, "does/not/exist.rs"),
        other => panic!("expected SelectorOutOfScope, got {:?}", other),
    }
}
