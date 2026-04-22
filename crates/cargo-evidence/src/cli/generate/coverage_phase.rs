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
    CoverageLevel, Diagnostic, Location, Profile, Severity, parse_llvm_cov_export,
};
use tempfile::TempDir;

use crate::cli::args::CoverageChoice;
use crate::cli::output::emit_jsonl;

/// Outcome of [`run_coverage_phase`]. The caller uses it to
/// decide whether to keep processing (skipped / emitted) or
/// short-circuit to a `GENERATE_FAIL` terminal (cert/record +
/// binary missing).
pub enum CoverageOutcome {
    /// Flag was `none` or profile-derived default was `none` —
    /// skipped entirely. No diagnostic emitted.
    Skipped,
    /// Coverage report written to bundle.
    Emitted,
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
    builder: &EvidenceBuilder,
    choice: CoverageChoice,
    profile: Profile,
    quiet: bool,
    jsonl_output: bool,
) -> Result<CoverageOutcome> {
    let levels = levels_for_choice(choice);
    if levels.is_empty() {
        return Ok(CoverageOutcome::Skipped);
    }

    let tmp = TempDir::new().context("creating coverage tempdir")?;
    let json_path = tmp.path().join("llvm-cov.json");

    match spawn_llvm_cov(&json_path) {
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
            anyhow::bail!("cargo-llvm-cov exited non-zero ({code})");
        }
        Err(LlvmCovSpawnError::Other(e)) => return Err(e),
    }

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

    emit_coverage_ok(&report, jsonl_output, quiet)?;

    Ok(CoverageOutcome::Emitted)
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

fn spawn_llvm_cov(json_out: &Path) -> Result<(), LlvmCovSpawnError> {
    let result = Command::new("cargo")
        .arg("llvm-cov")
        .arg("--workspace")
        .arg("--no-cfg-coverage")
        .arg("--json")
        .arg("--output-path")
        .arg(json_out)
        .env("CARGO_TERM_COLOR", "never")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output();
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
