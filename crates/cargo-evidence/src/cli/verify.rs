//! `cargo evidence verify`.

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::Serialize;

use evidence::{VerifyResult, verify_bundle_with_key};

use super::args::{EXIT_SUCCESS, EXIT_VERIFICATION_FAILURE};
use super::output::emit_json;

#[derive(Serialize)]
struct VerifyOutput {
    success: bool,
    bundle_path: String,
    checks: Vec<VerifyCheck>,
    error: Option<String>,
}

#[derive(Serialize)]
struct VerifyCheck {
    name: String,
    status: String,
    message: Option<String>,
}

/// Emit a verify failure envelope and return the given exit code.
///
/// Unifies the pattern that used to repeat five times in cmd_verify:
/// build a `VerifyOutput { success: false, ... error: Some(msg) }`,
/// emit JSON on --json or print a text line with the given prefix to
/// stderr (so e.g. "verify: FAIL - foo" vs "error: foo" vs "verify:
/// ERROR - foo" are all one call site). The caller controls the
/// text prefix and exit code.
fn fail_verify(
    json_output: bool,
    bundle_path: &std::path::Path,
    checks: Vec<VerifyCheck>,
    text_prefix: &str,
    msg: impl Into<String>,
    exit_code: i32,
) -> Result<i32> {
    let msg = msg.into();
    if json_output {
        emit_json(&VerifyOutput {
            success: false,
            bundle_path: bundle_path.display().to_string(),
            checks,
            error: Some(msg),
        })?;
    } else {
        eprintln!("{} {}", text_prefix, msg);
    }
    Ok(exit_code)
}

pub fn cmd_verify(
    bundle_path: PathBuf,
    strict: bool,
    verify_key: Option<PathBuf>,
    json_output: bool,
) -> Result<i32> {
    let mut checks = Vec::new();

    // Check bundle exists
    if !bundle_path.exists() {
        let err_msg = format!("bundle not found: {:?}", bundle_path);
        return fail_verify(
            json_output,
            &bundle_path,
            vec![VerifyCheck {
                name: "bundle_exists".to_string(),
                status: "fail".to_string(),
                message: Some(err_msg.clone()),
            }],
            "error:",
            err_msg,
            EXIT_VERIFICATION_FAILURE,
        );
    }

    checks.push(VerifyCheck {
        name: "bundle_exists".to_string(),
        status: "pass".to_string(),
        message: None,
    });

    // Strict mode: require BUNDLE.sig to exist
    if strict && !bundle_path.join("BUNDLE.sig").exists() && verify_key.is_none() {
        let err_msg = "strict mode: BUNDLE.sig not found and no --verify-key provided";
        return fail_verify(
            json_output,
            &bundle_path,
            vec![VerifyCheck {
                name: "bundle_signature".to_string(),
                status: "fail".to_string(),
                message: Some(err_msg.to_string()),
            }],
            "verify: FAIL -",
            err_msg,
            EXIT_VERIFICATION_FAILURE,
        );
    }

    // Load verify key if provided
    let key_bytes = match &verify_key {
        Some(path) => {
            Some(fs::read(path).with_context(|| format!("reading verify key from {:?}", path))?)
        }
        None => None,
    };

    // Run verification
    match verify_bundle_with_key(&bundle_path, key_bytes.as_deref()) {
        Ok(VerifyResult::Pass) => {
            checks.push(VerifyCheck {
                name: "bundle_integrity".to_string(),
                status: "pass".to_string(),
                message: None,
            });
            checks.push(VerifyCheck {
                name: "sha256sums".to_string(),
                status: "pass".to_string(),
                message: None,
            });

            if json_output {
                emit_json(&VerifyOutput {
                    success: true,
                    bundle_path: bundle_path.display().to_string(),
                    checks,
                    error: None,
                })?;
            } else {
                println!("verify: PASS - bundle {:?}", bundle_path);
            }
            Ok(EXIT_SUCCESS)
        }
        Ok(VerifyResult::Fail(errors)) => {
            let reason = errors
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("; ");
            // Map each VerifyError to its own VerifyCheck for granular JSON output
            for err in &errors {
                let name = match err {
                    evidence::VerifyError::UnexpectedFile(_) => "unexpected_file",
                    evidence::VerifyError::HmacFailure => "hmac_signature",
                    evidence::VerifyError::HashMismatch { .. } => "hash_mismatch",
                    evidence::VerifyError::MissingHashedFile(_) => "missing_file",
                    evidence::VerifyError::ContentHashMismatch { .. } => "content_hash",
                    evidence::VerifyError::UnsafePath(_) => "unsafe_path",
                    evidence::VerifyError::FormatError { .. } => "format_error",
                    evidence::VerifyError::CrossFileInconsistency { .. } => "cross_file_mismatch",
                    evidence::VerifyError::DeterministicHashMismatch { .. } => "deterministic_hash",
                    evidence::VerifyError::ManifestProjectionDrift { .. } => "manifest_projection",
                    evidence::VerifyError::TraceOutputNotHashed(_) => "trace_output_not_hashed",
                    evidence::VerifyError::TestSummaryMismatch { .. } => "test_summary_mismatch",
                    evidence::VerifyError::DalMapMismatch { .. } => "dal_map_mismatch",
                    evidence::VerifyError::DalMapOrphan { .. } => "dal_map_orphan",
                };
                checks.push(VerifyCheck {
                    name: name.to_string(),
                    status: "fail".to_string(),
                    message: Some(err.to_string()),
                });
            }

            fail_verify(
                json_output,
                &bundle_path,
                checks,
                "verify: FAIL -",
                reason,
                EXIT_VERIFICATION_FAILURE,
            )
        }
        Ok(VerifyResult::Skipped(reason)) => {
            // In strict mode, skipped checks are treated as failures
            let treat_as_fail = strict;
            checks.push(VerifyCheck {
                name: "bundle_integrity".to_string(),
                status: if treat_as_fail { "fail" } else { "skipped" }.to_string(),
                message: Some(reason.clone()),
            });

            if json_output {
                emit_json(&VerifyOutput {
                    success: !treat_as_fail,
                    bundle_path: bundle_path.display().to_string(),
                    checks,
                    error: if treat_as_fail {
                        Some(format!("strict mode: {}", reason))
                    } else {
                        None
                    },
                })?;
            } else if treat_as_fail {
                eprintln!("verify: FAIL (strict) - {}", reason);
            } else {
                println!("verify: SKIPPED - {}", reason);
            }
            Ok(if treat_as_fail {
                EXIT_VERIFICATION_FAILURE
            } else {
                EXIT_SUCCESS
            })
        }
        Err(e) => fail_verify(
            json_output,
            &bundle_path,
            checks,
            "verify: ERROR -",
            e.to_string(),
            EXIT_VERIFICATION_FAILURE,
        ),
    }
}
