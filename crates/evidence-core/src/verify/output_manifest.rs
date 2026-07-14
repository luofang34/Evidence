//! Verify-time fail-closed check on the output manifest.
//!
//! When a bundle records a test summary it compiled the workspace, so
//! it must also record the build's deliverables in `outputs_hashes.json`.
//! An empty output manifest on a bundle that built is the defect this
//! check rejects — deliverables produced but never attested. A
//! `--skip-tests` bundle
//! compiles nothing and legitimately has no outputs, so the check keys
//! on whether a build ran, not on the profile.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use super::errors::VerifyError;

/// Push [`VerifyError::OutputManifestEmpty`] when `build_ran` is true
/// but `outputs_hashes.json` parses to an empty object. A missing or
/// unparseable file is handled by the required-files / hash-manifest
/// checks upstream, so this check stays silent on those.
pub fn check_output_manifest(bundle: &Path, build_ran: bool, errors: &mut Vec<VerifyError>) {
    if !build_ran {
        return;
    }
    let path = bundle.join("outputs_hashes.json");
    let content = match fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => return,
    };
    let outputs: BTreeMap<String, String> = match serde_json::from_str(&content) {
        Ok(m) => m,
        Err(_) => return,
    };
    if outputs.is_empty() {
        errors.push(VerifyError::OutputManifestEmpty);
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn write(dir: &Path, body: &str) {
        fs::write(dir.join("outputs_hashes.json"), body).expect("write outputs");
    }

    #[test]
    fn empty_outputs_after_build_is_rejected() {
        let dir = tempfile::tempdir().expect("tempdir");
        write(dir.path(), "{}");
        let mut errors = Vec::new();
        check_output_manifest(dir.path(), true, &mut errors);
        assert!(matches!(
            errors.as_slice(),
            [VerifyError::OutputManifestEmpty]
        ));
    }

    #[test]
    fn non_empty_outputs_pass() {
        let dir = tempfile::tempdir().expect("tempdir");
        write(dir.path(), r#"{"target/debug/x":"abc"}"#);
        let mut errors = Vec::new();
        check_output_manifest(dir.path(), true, &mut errors);
        assert!(errors.is_empty());
    }

    #[test]
    fn skip_build_allows_empty_outputs() {
        let dir = tempfile::tempdir().expect("tempdir");
        write(dir.path(), "{}");
        let mut errors = Vec::new();
        check_output_manifest(dir.path(), false, &mut errors);
        assert!(errors.is_empty());
    }
}
