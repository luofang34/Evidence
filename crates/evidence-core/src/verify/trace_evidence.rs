//! Verify-time fail-closed check on bundle trace evidence
//! (LLR-107 / HLR-086 / SYS-036).
//!
//! A cert/record-profile bundle claims traceability. A bundle that
//! ships no trace requirement files, or ships files whose
//! requirement lists total zero, has no evidence behind that claim
//! — both are adoption states, and verify must fail closed on
//! them. The two [`VerifyError`] variants reuse the source-side
//! semantic codes (`TRACE_EVIDENCE_NOT_ADOPTED` /
//! `TRACE_EVIDENCE_EMPTY`) so a consumer sees the same code at
//! both ends of the pipeline.
//!
//! Dev-profile bundles keep their historical skip semantics: a dev
//! snapshot with no trace config is a legitimate debugging
//! artifact, so the check keys on the profile, mirroring
//! `check_bundle_completeness`'s cert/record gate.

use std::path::Path;

use super::errors::VerifyError;
use crate::policy::Profile;
use crate::trace::{DerivedFile, HlrFile, LlrFile, TestsFile, read_toml};

/// Push [`VerifyError::TraceEvidenceNotAdopted`] when a
/// cert/record-profile bundle contains none of the trace
/// requirement files, or [`VerifyError::TraceEvidenceEmpty`] when
/// at least one file is present but every requirement list is
/// empty. Unparseable files count as zero entries here — the hash
/// manifest upstream already flags corrupted content, so this
/// check stays silent on parse failures to avoid double findings.
pub fn check_trace_evidence(bundle: &Path, profile: &Profile, errors: &mut Vec<VerifyError>) {
    if !matches!(profile, Profile::Cert | Profile::Record) {
        return;
    }
    let trace_dir = bundle.join("trace");
    let mut any_file = false;
    let mut requirement_count = 0usize;

    let hlr_like = |name: &str, count: &mut usize, any: &mut bool| {
        let path = trace_dir.join(name);
        if path.is_file() {
            *any = true;
            if let Ok(f) = read_toml::<HlrFile>(&path) {
                *count += f.requirements.len();
            }
        }
    };
    hlr_like("sys.toml", &mut requirement_count, &mut any_file);
    hlr_like("hlr.toml", &mut requirement_count, &mut any_file);

    let llr_path = trace_dir.join("llr.toml");
    if llr_path.is_file() {
        any_file = true;
        if let Ok(f) = read_toml::<LlrFile>(&llr_path) {
            requirement_count += f.requirements.len();
        }
    }
    let tests_path = trace_dir.join("tests.toml");
    if tests_path.is_file() {
        any_file = true;
        if let Ok(f) = read_toml::<TestsFile>(&tests_path) {
            requirement_count += f.tests.len();
        }
    }
    let derived_path = trace_dir.join("derived.toml");
    if derived_path.is_file() {
        any_file = true;
        if let Ok(f) = read_toml::<DerivedFile>(&derived_path) {
            requirement_count += f.requirements.len();
        }
    }

    if !any_file {
        errors.push(VerifyError::TraceEvidenceNotAdopted {
            profile: profile.to_string(),
        });
    } else if requirement_count == 0 {
        errors.push(VerifyError::TraceEvidenceEmpty {
            profile: profile.to_string(),
        });
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

    const SCHEMA_V: &str = crate::schema_versions::TRACE;

    fn hlr_file_body(count: usize) -> String {
        let mut body = format!(
            "[schema]\nversion = \"{SCHEMA_V}\"\n\n[meta]\ndocument_id = \"HLR\"\nrevision = \"1\"\n"
        );
        for i in 0..count {
            body.push_str(&format!(
                "\n[[requirements]]\nuid = \"aaaaaaaa-0000-4000-8000-{i:012}\"\nid = \"HLR-{i}\"\ntitle = \"t\"\n"
            ));
        }
        body
    }

    /// Cert-profile bundle with no `trace/` files at all →
    /// not-adopted fires.
    #[test]
    fn cert_bundle_without_trace_files_is_not_adopted() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mut errors = Vec::new();
        check_trace_evidence(tmp.path(), &Profile::Cert, &mut errors);
        assert!(matches!(
            errors.as_slice(),
            [VerifyError::TraceEvidenceNotAdopted { .. }]
        ));
    }

    /// Cert-profile bundle with trace files but zero requirements
    /// → empty fires.
    #[test]
    fn cert_bundle_with_empty_trace_set_is_empty() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("trace")).unwrap();
        std::fs::write(tmp.path().join("trace").join("hlr.toml"), hlr_file_body(0)).unwrap();
        let mut errors = Vec::new();
        check_trace_evidence(tmp.path(), &Profile::Record, &mut errors);
        assert!(matches!(
            errors.as_slice(),
            [VerifyError::TraceEvidenceEmpty { .. }]
        ));
    }

    /// Non-empty trace set → no finding.
    #[test]
    fn cert_bundle_with_requirements_passes() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("trace")).unwrap();
        std::fs::write(tmp.path().join("trace").join("hlr.toml"), hlr_file_body(1)).unwrap();
        let mut errors = Vec::new();
        check_trace_evidence(tmp.path(), &Profile::Cert, &mut errors);
        assert!(errors.is_empty(), "got {errors:?}");
    }

    /// Dev profile keeps skip semantics even with no trace files.
    #[test]
    fn dev_bundle_skips_the_check() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mut errors = Vec::new();
        check_trace_evidence(tmp.path(), &Profile::Dev, &mut errors);
        assert!(errors.is_empty(), "got {errors:?}");
    }
}
