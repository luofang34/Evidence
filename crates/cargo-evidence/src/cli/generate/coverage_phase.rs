//! Phase 5b — structural coverage capture via cargo-llvm-cov.
//!
//! Runs between the plain `cargo test` phase and trace validation
//! when the user requests non-`none` coverage. Spawns
//! `cargo llvm-cov --workspace --json --output-path <tmp>`,
//! parses the export, and writes the typed
//! `bundle/coverage/coverage_summary.json` artifact. The bundle
//! walker in `write_sha256sums` picks up the new file
//! automatically at finalize time, so no builder state change is
//! needed.
//!
//! LLR-053.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result};
use evidence_core::bundle::EvidenceBuilder;
use evidence_core::{
    CoverageLevel, CoverageReport, Dal, DalCoverageThresholds, Diagnostic, Location, Measurement,
    Profile, Severity, parse_llvm_cov_export,
};
use tempfile::TempDir;

use crate::cli::args::CoverageChoice;
use crate::cli::output::emit_jsonl;

/// Outcome of [`run_coverage_phase`]. The caller uses it to
/// decide whether to keep processing (skipped / emitted) or
/// short-circuit to a `GENERATE_FAIL` terminal (cert/record +
/// binary missing, or cert/record + threshold violated).
pub enum CoverageOutcome {
    /// Flag was `none` or profile-derived default was `none` —
    /// skipped entirely. No diagnostic emitted.
    Skipped,
    /// Coverage report written to bundle; DAL thresholds met or
    /// not enforced (dev profile).
    Emitted,
    /// Coverage report written, but one or more DAL-policy
    /// thresholds were below the required minimum. Caller
    /// aborts with `GENERATE_FAIL` on cert/record; dev profile
    /// still produces a bundle (the diagnostic is visible in
    /// the JSONL stream).
    BelowThresholdCert,
    /// `cargo-llvm-cov` not on PATH under dev profile — Warning
    /// emitted, generation continues without coverage evidence.
    LlvmCovMissingDev,
    /// `cargo-llvm-cov` not on PATH under cert/record profile —
    /// Error emitted. Caller should abort with `GENERATE_FAIL`.
    LlvmCovMissingCert,
}

/// Resolve the effective coverage choice: explicit CLI flag wins,
/// else fall back to the profile-derived default (dev=none,
/// cert/record=branch).
pub fn resolve_choice(cli: Option<CoverageChoice>, profile: Profile) -> CoverageChoice {
    cli.unwrap_or(match profile {
        Profile::Dev => CoverageChoice::None,
        Profile::Cert | Profile::Record => CoverageChoice::Branch,
    })
}

/// Run Phase 5b.
pub fn run_coverage_phase(
    builder: &mut EvidenceBuilder,
    choice: CoverageChoice,
    profile: Profile,
    max_dal: Dal,
    quiet: bool,
    jsonl_output: bool,
) -> Result<CoverageOutcome> {
    let levels = levels_for_choice(choice);
    if levels.is_empty() {
        return Ok(CoverageOutcome::Skipped);
    }

    let tmp = TempDir::new().context("creating coverage tempdir")?;
    let json_path = tmp.path().join("llvm-cov.json");
    let lcov_path = tmp.path().join("llvm-cov.lcov");

    // Phase 1: run instrumented tests without emitting a report
    // (leaves profdata files for `report` to consume twice —
    // once per format, avoiding a second test run).
    match spawn_llvm_cov_no_report() {
        Ok(()) => {}
        Err(LlvmCovSpawnError::BinaryMissing) => {
            let is_cert = matches!(profile, Profile::Cert | Profile::Record);
            let severity = if is_cert {
                Severity::Error
            } else {
                Severity::Warning
            };
            emit_llvmcov_missing(severity, profile, jsonl_output, quiet)?;
            return Ok(if is_cert {
                CoverageOutcome::LlvmCovMissingCert
            } else {
                CoverageOutcome::LlvmCovMissingDev
            });
        }
        Err(LlvmCovSpawnError::NonZeroExit(code)) => {
            anyhow::bail!("cargo-llvm-cov --no-report exited non-zero ({code})");
        }
        Err(LlvmCovSpawnError::Other(e)) => return Err(e),
    }
    // Phase 2: extract both JSON + lcov from the cached profdata.
    // Neither can fail BinaryMissing here (we just succeeded with
    // the same binary) so we treat any failure as a hard error.
    spawn_llvm_cov_report(&["--json", "--output-path"], &json_path)?;
    spawn_llvm_cov_report(&["--lcov", "--output-path"], &lcov_path)?;

    let json = std::fs::read_to_string(&json_path).context("reading cargo-llvm-cov JSON output")?;
    let workspace_root = std::env::current_dir().context("reading current dir")?;
    let report = match parse_llvm_cov_export(&json, &levels, &workspace_root) {
        Ok(r) => r,
        Err(e) => {
            emit_parse_failed(&e.to_string(), jsonl_output, quiet)?;
            return Err(anyhow::anyhow!("cargo-llvm-cov JSON parse failed: {e}"));
        }
    };

    let coverage_dir = builder.bundle_dir().join("coverage");
    std::fs::create_dir_all(&coverage_dir).with_context(|| format!("creating {coverage_dir:?}"))?;
    let summary_path = coverage_dir.join("coverage_summary.json");
    std::fs::write(
        &summary_path,
        serde_json::to_string_pretty(&report).context("serializing CoverageReport")?,
    )
    .with_context(|| format!("writing {summary_path:?}"))?;
    let lcov_dest = coverage_dir.join("lcov.info");
    std::fs::copy(&lcov_path, &lcov_dest)
        .with_context(|| format!("copying {lcov_path:?} → {lcov_dest:?}"))?;

    emit_coverage_ok(&report, jsonl_output, quiet)?;

    // Store the report on the builder so the downstream
    // compliance phase reads aggregate percents for A-7
    // Obj-5/Obj-6 evaluation.
    builder.set_coverage_report(report.clone());

    // DAL-policy threshold enforcement (cert/record only — dev
    // profile surfaces the report but never blocks on it).
    let enforce_thresholds = matches!(profile, Profile::Cert | Profile::Record);
    if enforce_thresholds {
        let thresholds = max_dal.coverage_thresholds();
        let violations = threshold_violations(&report, thresholds);
        if !violations.is_empty() {
            for v in &violations {
                emit_below_threshold(v, jsonl_output, quiet)?;
            }
            return Ok(CoverageOutcome::BelowThresholdCert);
        }
    }

    Ok(CoverageOutcome::Emitted)
}

/// Observed aggregate percentage per-level in `report`, compared
/// against `thresholds`. One [`ThresholdViolation`] per
/// dimension that falls short.
#[derive(Debug, Clone)]
struct ThresholdViolation {
    dimension: &'static str,
    current_percent: f64,
    threshold_percent: u8,
}

fn threshold_violations(
    report: &CoverageReport,
    thresholds: DalCoverageThresholds,
) -> Vec<ThresholdViolation> {
    let mut violations = Vec::new();
    if let Some(min) = thresholds.statement_percent
        && let Some(m) = measurement_for(report, CoverageLevel::Statement)
    {
        let pct = aggregate_percent(m);
        if pct < f64::from(min) {
            violations.push(ThresholdViolation {
                dimension: "statement",
                current_percent: pct,
                threshold_percent: min,
            });
        }
    }
    if let Some(min) = thresholds.branch_percent
        && let Some(m) = measurement_for(report, CoverageLevel::Branch)
    {
        let pct = aggregate_percent(m);
        if pct < f64::from(min) {
            violations.push(ThresholdViolation {
                dimension: "branch",
                current_percent: pct,
                threshold_percent: min,
            });
        }
    }
    violations
}

fn measurement_for(report: &CoverageReport, level: CoverageLevel) -> Option<&Measurement> {
    report.measurements.iter().find(|m| m.level == level)
}

fn aggregate_percent(m: &Measurement) -> f64 {
    let total: u64 = m.per_file.iter().map(|f| f.lines.total).sum();
    if total == 0 {
        return 0.0;
    }
    let covered: u64 = m.per_file.iter().map(|f| f.lines.covered).sum();
    // Avoid lossy f64 conversion: both sides are typically well
    // under 2^53, so as-cast is fine in practice. The assertion
    // documents the expectation for future readers.
    let covered_f = covered as f64;
    let total_f = total as f64;
    (covered_f / total_f) * 100.0
}

fn emit_below_threshold(v: &ThresholdViolation, jsonl_output: bool, quiet: bool) -> Result<()> {
    let message = format!(
        "coverage below DAL threshold: {} = {:.2}%, required ≥ {}%",
        v.dimension, v.current_percent, v.threshold_percent
    );
    if jsonl_output {
        crate::cli::output::emit_jsonl(&Diagnostic {
            code: "COVERAGE_BELOW_THRESHOLD".to_string(),
            severity: Severity::Error,
            message,
            location: Some(Location {
                file: Some(PathBuf::from("coverage/coverage_summary.json")),
                ..Location::default()
            }),
            fix_hint: None,
            subcommand: Some("generate".to_string()),
            root_cause_uid: None,
        })?;
    } else if !quiet {
        eprintln!("coverage: ERROR: {message}");
    }
    Ok(())
}

fn levels_for_choice(choice: CoverageChoice) -> Vec<CoverageLevel> {
    match choice {
        CoverageChoice::None => Vec::new(),
        CoverageChoice::Line => vec![CoverageLevel::Statement],
        CoverageChoice::Branch => vec![CoverageLevel::Branch],
        CoverageChoice::All => vec![CoverageLevel::Statement, CoverageLevel::Branch],
    }
}

enum LlvmCovSpawnError {
    BinaryMissing,
    NonZeroExit(i32),
    Other(anyhow::Error),
}

/// Phase-1 spawn: instrumented test run, no report emitted.
/// Leaves profdata files in `target/llvm-cov-target` for the
/// `report` sub-invocations to consume.
fn spawn_llvm_cov_no_report() -> Result<(), LlvmCovSpawnError> {
    let result = Command::new("cargo")
        .arg("llvm-cov")
        .arg("--workspace")
        .arg("--no-cfg-coverage")
        .arg("--no-report")
        .env("CARGO_TERM_COLOR", "never")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output();
    interpret_output(result)
}

/// Phase-2 spawn: `cargo llvm-cov report` with a format flag.
/// Reads the cached profdata — fast (no rebuild, no test run).
fn spawn_llvm_cov_report(fmt_args: &[&str], output_path: &Path) -> Result<()> {
    let result = Command::new("cargo")
        .arg("llvm-cov")
        .arg("report")
        .args(fmt_args)
        .arg(output_path)
        .env("CARGO_TERM_COLOR", "never")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output();
    match interpret_output(result) {
        Ok(()) => Ok(()),
        Err(LlvmCovSpawnError::BinaryMissing) => {
            anyhow::bail!("cargo-llvm-cov vanished between --no-report and report phases")
        }
        Err(LlvmCovSpawnError::NonZeroExit(code)) => {
            anyhow::bail!("cargo llvm-cov report exited non-zero ({code})")
        }
        Err(LlvmCovSpawnError::Other(e)) => Err(e),
    }
}

fn interpret_output(
    result: std::io::Result<std::process::Output>,
) -> Result<(), LlvmCovSpawnError> {
    match result {
        Ok(out) if out.status.success() => Ok(()),
        Ok(out) => {
            // Non-zero exit — could be subcommand-missing ("no
            // such command: llvm-cov") even though `cargo`
            // itself was found. Detect via stderr.
            let stderr = String::from_utf8_lossy(&out.stderr);
            if stderr.contains("no such command: `llvm-cov`") {
                Err(LlvmCovSpawnError::BinaryMissing)
            } else {
                Err(LlvmCovSpawnError::NonZeroExit(
                    out.status.code().unwrap_or(-1),
                ))
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // `cargo` itself missing — treat as binary-missing
            // for the purposes of diagnostic emission. Rare in
            // practice (cargo is the entry point for the tool).
            Err(LlvmCovSpawnError::BinaryMissing)
        }
        Err(e) => Err(LlvmCovSpawnError::Other(
            anyhow::anyhow!(e).context("spawning cargo llvm-cov"),
        )),
    }
}

fn emit_llvmcov_missing(
    severity: Severity,
    profile: Profile,
    jsonl_output: bool,
    quiet: bool,
) -> Result<()> {
    let message = format!(
        "cargo-llvm-cov not on PATH; profile={} — install via \
         `cargo install cargo-llvm-cov` to capture structural coverage",
        profile
    );
    if jsonl_output {
        emit_jsonl(&Diagnostic {
            code: "COVERAGE_LLVMCOV_MISSING".to_string(),
            severity,
            message,
            location: None,
            fix_hint: None,
            subcommand: Some("generate".to_string()),
            root_cause_uid: None,
        })?;
    } else if !quiet {
        let tag = if severity == Severity::Error {
            "coverage: ERROR"
        } else {
            "coverage: warn"
        };
        eprintln!("{tag}: cargo-llvm-cov not on PATH; install via `cargo install cargo-llvm-cov`");
    }
    Ok(())
}

fn emit_parse_failed(detail: &str, jsonl_output: bool, quiet: bool) -> Result<()> {
    let message = format!("cargo-llvm-cov JSON parse failed: {detail}");
    if jsonl_output {
        emit_jsonl(&Diagnostic {
            code: "COVERAGE_PARSE_FAILED".to_string(),
            severity: Severity::Error,
            message,
            location: None,
            fix_hint: None,
            subcommand: Some("generate".to_string()),
            root_cause_uid: None,
        })?;
    } else if !quiet {
        eprintln!("coverage: ERROR: {message}");
    }
    Ok(())
}

fn emit_coverage_ok(
    report: &evidence_core::CoverageReport,
    jsonl_output: bool,
    quiet: bool,
) -> Result<()> {
    let summary = report
        .measurements
        .iter()
        .map(|m| {
            let files = m.per_file.len();
            let total_lines: u64 = m.per_file.iter().map(|f| f.lines.total).sum();
            let covered_lines: u64 = m.per_file.iter().map(|f| f.lines.covered).sum();
            format!(
                "{level:?}: {files} file(s), {covered_lines}/{total_lines} lines",
                level = m.level
            )
        })
        .collect::<Vec<_>>()
        .join("; ");
    let message = format!(
        "coverage written (engine={}); {}",
        report
            .measurements
            .first()
            .map(|m| m.engine_version.as_str())
            .unwrap_or("unknown"),
        summary
    );
    if jsonl_output {
        emit_jsonl(&Diagnostic {
            code: "COVERAGE_OK".to_string(),
            severity: Severity::Info,
            message,
            location: Some(Location {
                file: Some(PathBuf::from("coverage/coverage_summary.json")),
                ..Location::default()
            }),
            fix_hint: None,
            subcommand: Some("generate".to_string()),
            root_cause_uid: None,
        })?;
    } else if !quiet {
        eprintln!("coverage: {message}");
    }
    Ok(())
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

    #[test]
    fn resolve_choice_explicit_wins_over_profile_default() {
        assert_eq!(
            resolve_choice(Some(CoverageChoice::None), Profile::Cert),
            CoverageChoice::None
        );
        assert_eq!(
            resolve_choice(Some(CoverageChoice::Branch), Profile::Dev),
            CoverageChoice::Branch
        );
    }

    #[test]
    fn resolve_choice_defaults_by_profile_when_unset() {
        assert_eq!(resolve_choice(None, Profile::Dev), CoverageChoice::None);
        assert_eq!(resolve_choice(None, Profile::Cert), CoverageChoice::Branch);
        assert_eq!(
            resolve_choice(None, Profile::Record),
            CoverageChoice::Branch
        );
    }

    #[test]
    fn levels_for_choice_maps_as_expected() {
        assert!(levels_for_choice(CoverageChoice::None).is_empty());
        assert_eq!(
            levels_for_choice(CoverageChoice::Line),
            vec![CoverageLevel::Statement]
        );
        assert_eq!(
            levels_for_choice(CoverageChoice::Branch),
            vec![CoverageLevel::Branch]
        );
        assert_eq!(
            levels_for_choice(CoverageChoice::All),
            vec![CoverageLevel::Statement, CoverageLevel::Branch]
        );
    }
}
