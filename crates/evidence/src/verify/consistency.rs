//! Cross-layer consistency checks — step 6c/6d/6e of `verify_bundle`.
//!
//! Each check here catches tampering that can't be detected by the
//! HMAC envelope alone (e.g. an insider-with-key attack) by requiring
//! index.json fields to agree with independently-derivable data in
//! the signed content layer:
//!
//! - `check_trace_outputs_hashed` — every `trace_outputs[i]` appears in SHA256SUMS
//! - `check_test_summary` — `index.test_summary` equals a re-parse of captured stdout
//! - `check_dal_map` — `index.dal_map[crate]` equals `compliance/<crate>.json.dal`

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use crate::bundle::EvidenceIndex;

use super::errors::VerifyError;

/// Every path in `index.json.trace_outputs` must also appear in
/// `SHA256SUMS`. The earlier "file exists on disk" check is too weak
/// — a tamperer could add a phantom entry pointing at some other
/// hashed file and claim coverage it doesn't have. Binding trace
/// outputs to the content layer via SHA256SUMS closes that.
pub(super) fn check_trace_outputs_hashed(
    index: &EvidenceIndex,
    listed_files: &BTreeSet<String>,
    errors: &mut Vec<VerifyError>,
) {
    for trace_out in &index.trace_outputs {
        if !listed_files.contains(trace_out) {
            errors.push(VerifyError::TraceOutputNotHashed(trace_out.clone()));
        }
    }
}

/// Re-parse the captured cargo test stdout and require byte-level
/// agreement with `index.json.test_summary`. Catches:
///   1. a tamperer who rewrote test_summary counts in index.json;
///   2. tool drift where the generate-time parser and the verify-time
///      parser produce different summaries from the same stdout bytes
///      (a DO-330 tool-qualification concern).
///
/// Only fires when BOTH a captured stdout AND an
/// `index.json.test_summary` are present. A bundle generated with
/// `--skip-tests` legitimately carries neither.
pub(super) fn check_test_summary(
    bundle: &Path,
    index: &EvidenceIndex,
    errors: &mut Vec<VerifyError>,
) {
    let Some(index_ts) = index.test_summary.as_ref() else {
        return;
    };
    let stdout_rel = "tests/cargo_test_stdout.txt";
    let stdout_path = bundle.join(stdout_rel);
    if !stdout_path.exists() {
        return;
    }
    let captured = match fs::read_to_string(&stdout_path) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("verify: cannot re-parse {}: {}", stdout_rel, e);
            return;
        }
    };
    match crate::bundle::parse_cargo_test_output(&captured) {
        Some(parsed) => {
            let cases: [(&'static str, u32, u32); 5] = [
                ("total", index_ts.total, parsed.total),
                ("passed", index_ts.passed, parsed.passed),
                ("failed", index_ts.failed, parsed.failed),
                ("ignored", index_ts.ignored, parsed.ignored),
                ("filtered_out", index_ts.filtered_out, parsed.filtered_out),
            ];
            for (field, idx_v, parsed_v) in cases {
                if idx_v != parsed_v {
                    errors.push(VerifyError::TestSummaryMismatch {
                        field,
                        index_value: idx_v.to_string(),
                        parsed_value: parsed_v.to_string(),
                    });
                }
            }
        }
        None => {
            errors.push(VerifyError::TestSummaryMismatch {
                field: "parse",
                index_value: format!(
                    "total={} passed={} failed={}",
                    index_ts.total, index_ts.passed, index_ts.failed
                ),
                parsed_value: "no `test result:` line found in cargo_test_stdout.txt".to_string(),
            });
        }
    }
}

/// `dal_map ↔ compliance/<crate>.json` cross-check. Each crate named
/// in `index.json.dal_map` must have a matching compliance report
/// whose `dal` field agrees; extra `compliance/*.json` files (or
/// missing `dal_map` entries) are flagged as orphans. Without this,
/// a holder with the HMAC key could demote a crate's DAL in
/// index.json while leaving the qualifying compliance artifact
/// untouched.
pub(super) fn check_dal_map(bundle: &Path, index: &EvidenceIndex, errors: &mut Vec<VerifyError>) {
    let compliance_dir = bundle.join("compliance");

    // index → compliance direction.
    for (crate_name, index_dal) in &index.dal_map {
        let rep_path = compliance_dir.join(format!("{}.json", crate_name));
        if !rep_path.exists() {
            errors.push(VerifyError::DalMapOrphan {
                crate_name: crate_name.clone(),
                detail: format!("compliance/{}.json missing", crate_name),
            });
            continue;
        }
        match fs::read_to_string(&rep_path)
            .map(|s| serde_json::from_str::<crate::compliance::ComplianceReport>(&s))
        {
            Ok(Ok(report)) => {
                if report.dal != *index_dal {
                    errors.push(VerifyError::DalMapMismatch {
                        crate_name: crate_name.clone(),
                        index_value: index_dal.clone(),
                        compliance_value: report.dal.clone(),
                    });
                }
            }
            Ok(Err(e)) => {
                errors.push(VerifyError::DalMapOrphan {
                    crate_name: crate_name.clone(),
                    detail: format!("compliance/{}.json parse error: {}", crate_name, e),
                });
            }
            Err(e) => {
                errors.push(VerifyError::DalMapOrphan {
                    crate_name: crate_name.clone(),
                    detail: format!("compliance/{}.json read error: {}", crate_name, e),
                });
            }
        }
    }

    // compliance → index direction: any `compliance/*.json` not
    // referenced in dal_map is also a drift.
    if compliance_dir.is_dir() {
        if let Ok(entries) = fs::read_dir(&compliance_dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.extension().and_then(|e| e.to_str()) != Some("json") {
                    continue;
                }
                let stem = match p.file_stem().and_then(|s| s.to_str()) {
                    Some(s) => s.to_string(),
                    None => continue,
                };
                if !index.dal_map.contains_key(&stem) {
                    errors.push(VerifyError::DalMapOrphan {
                        crate_name: stem.clone(),
                        detail: format!("compliance/{}.json has no entry in index.dal_map", stem),
                    });
                }
            }
        }
    }
}
