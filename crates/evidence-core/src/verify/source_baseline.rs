//! Verify-time fail-closed check on the source baseline.
//!
//! A bundle that declares in-scope Cargo packages claims a source
//! baseline: the tracked sources of those packages plus the workspace-
//! control inputs. If it declares a scope yet records an empty
//! `inputs_hashes.json`, that is the #138 defect — a claimed baseline
//! that captured nothing. This is the verify-side guard against a
//! regression that reintroduces it (the generate-side guard lives in
//! `bundle::input_scope`).
//!
//! A bundle with no declared scope (an empty boundary) has no baseline
//! to capture, so an empty inputs map is legitimate there and is left
//! alone — the check keys on the claim, not the profile.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use super::errors::VerifyError;

/// Push [`VerifyError::SourceBaselineEmpty`] when the bundle declares
/// in-scope packages (`declares_scope`) but `inputs_hashes.json` parses
/// to an empty object. A missing or unparseable file is handled by the
/// required-files / hash-manifest checks upstream, so this check stays
/// silent on those to avoid duplicate findings.
pub fn check_source_baseline(bundle: &Path, declares_scope: bool, errors: &mut Vec<VerifyError>) {
    if !declares_scope {
        return;
    }
    let path = bundle.join("inputs_hashes.json");
    let content = match fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => return,
    };
    let inputs: BTreeMap<String, String> = match serde_json::from_str(&content) {
        Ok(m) => m,
        Err(_) => return,
    };
    if inputs.is_empty() {
        errors.push(VerifyError::SourceBaselineEmpty);
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn write(dir: &Path, body: &str) {
        fs::write(dir.join("inputs_hashes.json"), body).expect("write inputs");
    }

    #[test]
    fn empty_object_is_rejected() {
        let dir = tempfile::tempdir().expect("tempdir");
        write(dir.path(), "{}");
        let mut errors = Vec::new();
        check_source_baseline(dir.path(), true, &mut errors);
        assert!(matches!(
            errors.as_slice(),
            [VerifyError::SourceBaselineEmpty]
        ));
    }

    #[test]
    fn non_empty_object_passes() {
        let dir = tempfile::tempdir().expect("tempdir");
        write(dir.path(), r#"{"Cargo.toml":"abc123"}"#);
        let mut errors = Vec::new();
        check_source_baseline(dir.path(), true, &mut errors);
        assert!(errors.is_empty());
    }

    #[test]
    fn empty_scope_allows_empty_inputs() {
        let dir = tempfile::tempdir().expect("tempdir");
        write(dir.path(), "{}");
        let mut errors = Vec::new();
        check_source_baseline(dir.path(), false, &mut errors);
        assert!(errors.is_empty());
    }

    #[test]
    fn missing_file_is_silent_here() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut errors = Vec::new();
        check_source_baseline(dir.path(), true, &mut errors);
        assert!(errors.is_empty());
    }
}
