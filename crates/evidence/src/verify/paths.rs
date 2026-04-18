//! Path safety + bundle-file allow-lists shared by every verification pass.

/// Check whether a relative path from SHA256SUMS or trace_outputs is safe
/// to join onto the bundle root.
///
/// Rejects absolute paths, `..` components, and Windows drive prefixes.
/// This prevents a crafted SHA256SUMS from causing the verifier to read
/// files outside the bundle directory.
pub(super) fn is_safe_bundle_path(filename: &str) -> bool {
    use std::path::{Component, Path};
    let path = Path::new(filename);
    if path.is_absolute() {
        return false;
    }
    for component in path.components() {
        match component {
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return false,
            _ => {}
        }
    }
    // Also reject leading backslash (Windows UNC without drive letter)
    !filename.starts_with('\\')
}

/// Required files that must exist in every evidence bundle.
pub const REQUIRED_FILES: &[&str] = &[
    "index.json",
    "inputs_hashes.json",
    "outputs_hashes.json",
    "commands.json",
    "env.json",
    "deterministic-manifest.json",
    "SHA256SUMS",
];

/// Files that are allowed in a bundle but not listed in SHA256SUMS.
pub(super) const KNOWN_META_FILES: &[&str] = &["index.json", "SHA256SUMS", "BUNDLE.sig"];

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
    fn test_required_files_list() {
        assert!(REQUIRED_FILES.contains(&"index.json"));
        assert!(REQUIRED_FILES.contains(&"SHA256SUMS"));
        assert!(REQUIRED_FILES.contains(&"deterministic-manifest.json"));
        assert_eq!(REQUIRED_FILES.len(), 7);
    }

    #[test]
    fn test_is_safe_bundle_path_valid() {
        assert!(is_safe_bundle_path("env.json"));
        assert!(is_safe_bundle_path("tests/cargo_test_stdout.txt"));
        assert!(is_safe_bundle_path("trace/matrix.md"));
        assert!(is_safe_bundle_path("sub/dir/file.txt"));
    }

    #[test]
    fn test_is_safe_bundle_path_rejects_traversal() {
        assert!(!is_safe_bundle_path("../../../etc/passwd"));
        assert!(!is_safe_bundle_path("sub/../../../etc/shadow"));
        assert!(!is_safe_bundle_path(".."));
    }

    #[test]
    fn test_is_safe_bundle_path_rejects_absolute() {
        assert!(!is_safe_bundle_path("/etc/passwd"));
        assert!(!is_safe_bundle_path("/tmp/file.txt"));
    }

    #[test]
    fn test_is_safe_bundle_path_rejects_backslash_prefix() {
        assert!(!is_safe_bundle_path("\\\\server\\share\\file"));
    }
}
