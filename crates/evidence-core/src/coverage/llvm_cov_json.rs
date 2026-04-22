//! Parser for `cargo-llvm-cov --json`'s export format.
//!
//! Shape we consume (trimmed to fields the tool actually reads):
//!
//! ```json
//! {
//!   "type": "llvm.coverage.json.export",
//!   "version": "3.1.0",
//!   "cargo_llvm_cov": {"version": "0.8.5", "manifest_path": "..."},
//!   "data": [
//!     {
//!       "files": [
//!         {
//!           "filename": "/abs/path/src/lib.rs",
//!           "summary": {
//!             "lines":    {"count": 7,  "covered": 7,  "percent": 100.0},
//!             "branches": {"count": 0,  "covered": 0,  "percent":   0.0},
//!             ...
//!           }
//!         }
//!       ]
//!     }
//!   ]
//! }
//! ```
//!
//! Filenames are absolute on disk; the parser normalizes them
//! against a workspace root so `coverage/coverage_summary.json`
//! is cross-host deterministic. Files outside the workspace are
//! dropped (workspace-scoped cert evidence only — tests dragging
//! in `~/.cargo/registry/…` are noise for the DO-178C case).

use std::path::Path;

use serde::Deserialize;
use thiserror::Error;

use super::report::{CoverageLevel, CoverageReport, FileMeasurement, LineCoverage, Measurement};
use crate::schema_versions;

/// Errors returned by [`parse_llvm_cov_export`].
#[derive(Debug, Error)]
pub enum LlvmCovParseError {
    /// JSON text failed to parse at the textual level.
    #[error("invalid cargo-llvm-cov JSON: {0}")]
    InvalidJson(#[from] serde_json::Error),
    /// JSON parsed but shape did not match llvm-cov's export
    /// schema — wrong `type` field, missing top-level key, etc.
    #[error("unexpected cargo-llvm-cov export shape: {0}")]
    UnexpectedShape(String),
}

#[derive(Deserialize)]
struct ExportTop {
    #[serde(rename = "type")]
    type_field: Option<String>,
    data: Vec<ExportDatum>,
    cargo_llvm_cov: Option<CargoLlvmCovMeta>,
}

#[derive(Deserialize)]
struct CargoLlvmCovMeta {
    version: Option<String>,
}

#[derive(Deserialize)]
struct ExportDatum {
    files: Vec<ExportFile>,
}

#[derive(Deserialize)]
struct ExportFile {
    filename: String,
    summary: ExportFileSummary,
}

#[derive(Deserialize)]
struct ExportFileSummary {
    lines: CountCovered,
    branches: CountCovered,
}

#[derive(Deserialize)]
struct CountCovered {
    count: u64,
    covered: u64,
}

/// Parse a cargo-llvm-cov JSON export into a typed
/// [`CoverageReport`], emitting one [`Measurement`] per requested
/// [`CoverageLevel`]. `workspace_root` is used to normalize
/// absolute filenames to workspace-relative paths; files outside
/// the workspace are dropped silently.
pub fn parse_llvm_cov_export(
    json: &str,
    levels: &[CoverageLevel],
    workspace_root: &Path,
) -> Result<CoverageReport, LlvmCovParseError> {
    let top: ExportTop = serde_json::from_str(json)?;
    if let Some(t) = &top.type_field
        && t != "llvm.coverage.json.export"
    {
        return Err(LlvmCovParseError::UnexpectedShape(format!(
            "expected type='llvm.coverage.json.export', got '{t}'"
        )));
    }
    let engine_version = top
        .cargo_llvm_cov
        .and_then(|m| m.version)
        .unwrap_or_else(|| "unknown".to_string());

    // Collect files across all `data` entries. llvm-cov's shape
    // is a one-element `data` array in practice; treat it as a
    // list to survive any future plural form.
    let mut per_file_lines: Vec<FileMeasurement> = Vec::new();
    let mut per_file_branches: Vec<FileMeasurement> = Vec::new();
    for datum in &top.data {
        for f in &datum.files {
            let Some(rel) = relative_to_workspace(&f.filename, workspace_root) else {
                continue;
            };
            per_file_lines.push(FileMeasurement {
                path: rel.clone(),
                lines: LineCoverage {
                    covered: f.summary.lines.covered,
                    total: f.summary.lines.count,
                },
                decisions: vec![],
                conditions: vec![],
            });
            per_file_branches.push(FileMeasurement {
                path: rel,
                lines: LineCoverage {
                    covered: f.summary.lines.covered,
                    total: f.summary.lines.count,
                },
                decisions: decisions_from_branch_summary(&f.summary.branches),
                conditions: vec![],
            });
        }
    }
    per_file_lines.sort_by(|a, b| a.path.cmp(&b.path));
    per_file_branches.sort_by(|a, b| a.path.cmp(&b.path));

    let mut measurements = Vec::with_capacity(levels.len());
    for &level in levels {
        let per_file = match level {
            CoverageLevel::Statement => per_file_lines.clone(),
            CoverageLevel::Branch => per_file_branches.clone(),
            // Reserved variants — emit an empty measurement so
            // the wire contract carries the level but auditors
            // see zero files claimed. Keeps future MC/DC
            // additions additive without downstream breakage.
            CoverageLevel::PatternDecision | CoverageLevel::Mcdc => Vec::new(),
        };
        measurements.push(Measurement {
            level,
            engine: "llvm-cov".to_string(),
            engine_version: engine_version.clone(),
            per_file,
        });
    }

    Ok(CoverageReport {
        schema_version: schema_versions::COVERAGE.to_string(),
        measurements,
    })
}

/// Collapse llvm-cov's file-level branch summary into a single
/// [`crate::coverage::DecisionCoverage`] record. We lose per-branch
/// location data here; the raw lcov passthrough file keeps the
/// detail for manual inspection. A richer per-branch model lands
/// when `regions` / `branches` array mining is added (v2 work).
fn decisions_from_branch_summary(summary: &CountCovered) -> Vec<super::report::DecisionCoverage> {
    if summary.count == 0 {
        return Vec::new();
    }
    vec![super::report::DecisionCoverage {
        id: format!("file-summary:{}/{}", summary.covered, summary.count),
        covered: summary.covered == summary.count,
        kind: "branch".to_string(),
    }]
}

/// Convert `abs_path` to a workspace-relative path with forward
/// slashes. Returns `None` if the path is not under
/// `workspace_root`.
fn relative_to_workspace(abs_path: &str, workspace_root: &Path) -> Option<String> {
    let abs = Path::new(abs_path);
    let rel = abs.strip_prefix(workspace_root).ok()?;
    Some(
        rel.components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/"),
    )
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

    /// Sample captured from running `cargo llvm-cov --json` on a
    /// trivial library crate. Exercises the happy path for both
    /// `type` + `cargo_llvm_cov.version` top-level fields and a
    /// single-file summary.
    const TRIVIAL_EXPORT: &str = r#"{
        "type": "llvm.coverage.json.export",
        "version": "3.1.0",
        "cargo_llvm_cov": {"version": "0.8.5", "manifest_path": "/workspace/Cargo.toml"},
        "data": [{
            "files": [{
                "filename": "/workspace/crates/a/src/lib.rs",
                "summary": {
                    "lines":    {"count": 10, "covered": 7, "percent": 70.0},
                    "branches": {"count": 4,  "covered": 3, "percent": 75.0},
                    "functions":{"count": 2,  "covered": 2, "percent": 100.0},
                    "regions":  {"count": 5,  "covered": 4, "percent": 80.0},
                    "mcdc":     {"count": 0,  "covered": 0, "percent": 0.0},
                    "instantiations":{"count": 2, "covered": 2, "percent": 100.0}
                }
            }]
        }]
    }"#;

    #[test]
    fn parses_trivial_export() {
        let report = parse_llvm_cov_export(
            TRIVIAL_EXPORT,
            &[CoverageLevel::Statement],
            Path::new("/workspace"),
        )
        .unwrap();
        assert_eq!(report.schema_version, crate::schema_versions::COVERAGE);
        assert_eq!(report.measurements.len(), 1);
        let m = &report.measurements[0];
        assert_eq!(m.level, CoverageLevel::Statement);
        assert_eq!(m.engine, "llvm-cov");
        assert_eq!(m.engine_version, "0.8.5");
        assert_eq!(m.per_file.len(), 1);
        assert_eq!(m.per_file[0].path, "crates/a/src/lib.rs");
        assert_eq!(m.per_file[0].lines.covered, 7);
        assert_eq!(m.per_file[0].lines.total, 10);
        // Statement level: no decisions or conditions.
        assert!(m.per_file[0].decisions.is_empty());
    }

    #[test]
    fn normalizes_absolute_paths_to_workspace_relative() {
        let report = parse_llvm_cov_export(
            TRIVIAL_EXPORT,
            &[CoverageLevel::Statement],
            Path::new("/workspace"),
        )
        .unwrap();
        // Path must have lost the `/workspace/` prefix and use
        // forward slashes regardless of host.
        assert_eq!(
            report.measurements[0].per_file[0].path,
            "crates/a/src/lib.rs"
        );
    }

    #[test]
    fn statement_and_branch_levels_are_separate_measurements() {
        let report = parse_llvm_cov_export(
            TRIVIAL_EXPORT,
            &[CoverageLevel::Statement, CoverageLevel::Branch],
            Path::new("/workspace"),
        )
        .unwrap();
        assert_eq!(report.measurements.len(), 2);
        assert_eq!(report.measurements[0].level, CoverageLevel::Statement);
        assert_eq!(report.measurements[1].level, CoverageLevel::Branch);
        // Branch-level file carries a decision summary; statement-
        // level file does not.
        assert!(report.measurements[0].per_file[0].decisions.is_empty());
        assert_eq!(
            report.measurements[1].per_file[0].decisions.len(),
            1,
            "one file-summary decision record expected"
        );
    }

    #[test]
    fn files_outside_workspace_are_dropped() {
        let report = parse_llvm_cov_export(
            TRIVIAL_EXPORT,
            &[CoverageLevel::Statement],
            Path::new("/not-workspace"),
        )
        .unwrap();
        assert_eq!(report.measurements.len(), 1);
        assert!(
            report.measurements[0].per_file.is_empty(),
            "files outside workspace must be dropped",
        );
    }

    #[test]
    fn reserved_levels_emit_empty_measurements() {
        let report = parse_llvm_cov_export(
            TRIVIAL_EXPORT,
            &[CoverageLevel::Mcdc, CoverageLevel::PatternDecision],
            Path::new("/workspace"),
        )
        .unwrap();
        assert_eq!(report.measurements.len(), 2);
        assert!(report.measurements[0].per_file.is_empty());
        assert!(report.measurements[1].per_file.is_empty());
    }

    #[test]
    fn wrong_type_field_rejected() {
        let bad = r#"{
            "type": "llvm.other.format",
            "data": [{"files": []}]
        }"#;
        let err =
            parse_llvm_cov_export(bad, &[CoverageLevel::Statement], Path::new("/")).unwrap_err();
        assert!(
            matches!(err, LlvmCovParseError::UnexpectedShape(_)),
            "got {err:?}"
        );
    }

    #[test]
    fn invalid_json_rejected() {
        let err = parse_llvm_cov_export("not json", &[CoverageLevel::Statement], Path::new("/"))
            .unwrap_err();
        assert!(
            matches!(err, LlvmCovParseError::InvalidJson(_)),
            "got {err:?}"
        );
    }
}
