//! Verify-side reverse check for the per-test ↔ LLR
//! bidirectional traceability loop: every test-verified LLR in
//! `bundle/trace/llr.toml` must have at least one
//! `tests/test_outcomes.jsonl` record whose `requirement_uids`
//! contains the LLR's `uid`.
//!
//! Forward direction — enriching records with
//! `requirement_uids` — lives in `cli/generate/test_outcomes.rs`
//! and `trace::test_backlinks`. LLR-052 owns both halves.
//!
//! Skip conditions (silently, not errors):
//!
//! - `tests/test_outcomes.jsonl` absent: older bundle (predates
//!   per-test outcome capture) or `--skip-tests` was passed.
//! - `bundle/trace/llr.toml` absent: no trace data was copied
//!   (legal for a dev-profile snapshot with no trace config).
//! - LLR has `uid = None`: pre-UUID-backfill trace data; can't
//!   cross-reference.
//! - LLR's `verification_methods` doesn't include `"test"`:
//!   review- or analysis-only LLRs legitimately have no test
//!   record.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use super::errors::VerifyError;

/// Push a [`VerifyError::LlrTestSelectorUnresolved`] for every
/// LLR in `bundle/trace/llr.toml` that declares
/// `verification_methods` including `"test"`, has a UID, but
/// isn't referenced by any record's `requirement_uids` in
/// `tests/test_outcomes.jsonl`.
pub fn check_llr_test_selectors(bundle: &Path, errors: &mut Vec<VerifyError>) {
    let jsonl_path = bundle.join("tests").join("test_outcomes.jsonl");
    if !jsonl_path.exists() {
        return;
    }
    let llr_path = bundle.join("trace").join("llr.toml");
    if !llr_path.exists() {
        return;
    }

    let jsonl = match fs::read_to_string(&jsonl_path) {
        Ok(s) => s,
        Err(_) => return,
    };
    let mut claimed_uids: BTreeSet<String> = BTreeSet::new();
    for line in jsonl.lines().filter(|l| !l.trim().is_empty()) {
        let rec: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if let Some(arr) = rec.get("requirement_uids").and_then(|v| v.as_array()) {
            for uid in arr {
                if let Some(s) = uid.as_str() {
                    claimed_uids.insert(s.to_string());
                }
            }
        }
    }

    let llr_text = match fs::read_to_string(&llr_path) {
        Ok(s) => s,
        Err(_) => return,
    };
    let llr_file: crate::trace::LlrFile = match toml::from_str(&llr_text) {
        Ok(f) => f,
        Err(_) => return,
    };

    for llr in &llr_file.requirements {
        let Some(uid) = llr.uid.as_ref().filter(|u| !u.is_empty()) else {
            continue;
        };
        if !llr
            .verification_methods
            .iter()
            .any(|m| m.eq_ignore_ascii_case("test"))
        {
            continue;
        }
        if !claimed_uids.contains(uid) {
            errors.push(VerifyError::LlrTestSelectorUnresolved {
                llr_uid: uid.clone(),
                llr_id: llr.id.clone(),
            });
        }
    }
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
    use tempfile::TempDir;

    fn seed_bundle(llrs_toml: &str, outcomes_jsonl: Option<&str>) -> TempDir {
        let tmp = TempDir::new().expect("tempdir");
        fs::create_dir_all(tmp.path().join("trace")).unwrap();
        fs::write(tmp.path().join("trace").join("llr.toml"), llrs_toml).unwrap();
        if let Some(jsonl) = outcomes_jsonl {
            fs::create_dir_all(tmp.path().join("tests")).unwrap();
            fs::write(tmp.path().join("tests").join("test_outcomes.jsonl"), jsonl).unwrap();
        }
        tmp
    }

    fn llr_with_one_test_verified() -> String {
        format!(
            r#"
[schema]
version = "{ver}"

[meta]
document_id = "LLR"
revision = "1.0"

[[requirements]]
uid = "00000000-0000-0000-0000-0000000000aa"
id = "LLR-TEST-1"
title = "Test-verified LLR"
owner = "tool"
traces_to = ["hlr-u"]
verification_methods = ["test"]

[[requirements]]
uid = "00000000-0000-0000-0000-0000000000bb"
id = "LLR-REVIEW-1"
title = "Review-only LLR"
owner = "tool"
traces_to = ["hlr-u"]
verification_methods = ["review"]
"#,
            ver = crate::schema_versions::TRACE
        )
    }

    /// Dangling LLR (test-verified, uid not in any record's
    /// requirement_uids) fires the error.
    #[test]
    fn llr_with_no_matching_test_outcome_fires_unresolved() {
        let tmp = seed_bundle(
            llr_with_one_test_verified().as_str(),
            Some(r#"{"name":"f","module_path":"m","passed":true,"ignored":false}"#),
        );
        let mut errors = Vec::new();
        check_llr_test_selectors(tmp.path(), &mut errors);
        let uids: Vec<&str> = errors
            .iter()
            .filter_map(|e| match e {
                VerifyError::LlrTestSelectorUnresolved { llr_uid, .. } => Some(llr_uid.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(uids, vec!["00000000-0000-0000-0000-0000000000aa"]);
    }

    /// LLR uid present in a record's requirement_uids → no error.
    #[test]
    fn llr_present_in_requirement_uids_passes() {
        let tmp = seed_bundle(
            llr_with_one_test_verified().as_str(),
            Some(
                r#"{"name":"f","module_path":"m","passed":true,"ignored":false,"requirement_uids":["00000000-0000-0000-0000-0000000000aa"]}"#,
            ),
        );
        let mut errors = Vec::new();
        check_llr_test_selectors(tmp.path(), &mut errors);
        assert!(errors.is_empty(), "uid present → no error; got {errors:?}");
    }

    /// Review-only LLR (`verification_methods` lacks `"test"`)
    /// doesn't need test-outcome coverage — skip cleanly.
    #[test]
    fn review_only_llr_is_not_checked() {
        // Build an LLR file with only the review-only entry.
        let toml = format!(
            r#"
[schema]
version = "{ver}"

[meta]
document_id = "LLR"
revision = "1.0"

[[requirements]]
uid = "00000000-0000-0000-0000-0000000000bb"
id = "LLR-REVIEW-1"
title = "Review-only LLR"
owner = "tool"
traces_to = ["hlr-u"]
verification_methods = ["review"]
"#,
            ver = crate::schema_versions::TRACE
        );
        let tmp = seed_bundle(&toml, Some(""));
        let mut errors = Vec::new();
        check_llr_test_selectors(tmp.path(), &mut errors);
        assert!(errors.is_empty());
    }

    /// Bundle without `tests/test_outcomes.jsonl` (older-bundle
    /// or --skip-tests) skips the check silently.
    #[test]
    fn absent_test_outcomes_skips_check() {
        let tmp = seed_bundle(llr_with_one_test_verified().as_str(), None);
        let mut errors = Vec::new();
        check_llr_test_selectors(tmp.path(), &mut errors);
        assert!(errors.is_empty());
    }

    /// Empty-string uid is treated the same as `uid = None`:
    /// continue past the entry rather than look for "" in the
    /// claimed-uid set. Defense-in-depth against corrupt trace
    /// data — UUID backfill policy makes this impossible in
    /// practice but the guard is cheap.
    #[test]
    fn llr_with_empty_uid_is_skipped_not_false_fired() {
        let toml_body = format!(
            r#"
[schema]
version = "{ver}"

[meta]
document_id = "LLR"
revision = "1.0"

[[requirements]]
uid = ""
id = "LLR-EMPTY-UID"
title = "Corrupt LLR with empty uid"
owner = "tool"
traces_to = ["hlr-u"]
verification_methods = ["test"]
"#,
            ver = crate::schema_versions::TRACE
        );
        let tmp = seed_bundle(
            &toml_body,
            Some(r#"{"name":"f","module_path":"m","passed":true,"ignored":false}"#),
        );
        let mut errors = Vec::new();
        check_llr_test_selectors(tmp.path(), &mut errors);
        assert!(
            errors.is_empty(),
            "empty uid must not trigger the unresolved error; got {errors:?}"
        );
    }
}
