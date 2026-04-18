//! Cryptographic hashing utilities.
//!
//! This module provides consistent hashing functions for
//! computing digests of files and data. Uses BTreeMap for
//! deterministic ordering in hash collections.

use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::diagnostic::{DiagnosticCode, Location, Severity};
use crate::util::normalize_bundle_path;

/// Errors returned by the hashing helpers in this module.
#[derive(Debug, Error)]
pub enum HashError {
    /// Failed to open `path` for hashing.
    #[error("opening {path:?}")]
    Open {
        /// File whose open failed.
        path: PathBuf,
        /// Underlying OS error.
        #[source]
        source: io::Error,
    },
    /// Failed to read bytes from `path` into the hasher.
    #[error("reading {path:?}")]
    Read {
        /// File whose streaming read failed.
        path: PathBuf,
        /// Underlying OS error.
        #[source]
        source: io::Error,
    },
    /// Failed to write `SHA256SUMS` at `path`.
    #[error("writing {path:?}")]
    Write {
        /// Output file whose write failed.
        path: PathBuf,
        /// Underlying OS error.
        #[source]
        source: io::Error,
    },
    /// A file walked during `write_sha256sums` raised an error.
    #[error("walking bundle tree")]
    Walk(#[source] walkdir::Error),
    /// `hash_file_relative_into` was given a path outside `base`, so
    /// `strip_prefix` failed.
    #[error("{path:?} is not under base {base:?}")]
    NotUnderBase {
        /// Path given by the caller.
        path: PathBuf,
        /// Base the path was expected to live under.
        base: PathBuf,
    },
    /// `hash_file_relative_into` was given a non-UTF-8 path; bundle
    /// JSON (`SHA256SUMS`, `index.json.trace_outputs`) only carries
    /// UTF-8 path strings, so non-UTF-8 is rejected up front instead
    /// of silently `to_string_lossy`-mangling.
    #[error("non-UTF-8 path: {path:?}")]
    NonUtf8Path {
        /// Offending non-UTF-8 path.
        path: PathBuf,
    },
}

impl DiagnosticCode for HashError {
    fn code(&self) -> &'static str {
        // Exhaustive match: adding a new HashError variant without a
        // stable code here fails compilation — Schema Rule 3.
        match self {
            HashError::Open { .. } => "HASH_OPEN_FAILED",
            HashError::Read { .. } => "HASH_READ_FAILED",
            HashError::Write { .. } => "HASH_WRITE_FAILED",
            HashError::Walk(_) => "HASH_WALK_FAILED",
            HashError::NotUnderBase { .. } => "HASH_NOT_UNDER_BASE",
            HashError::NonUtf8Path { .. } => "HASH_NON_UTF8_PATH",
        }
    }

    fn severity(&self) -> Severity {
        Severity::Error
    }

    fn location(&self) -> Option<Location> {
        let file = match self {
            HashError::Open { path, .. }
            | HashError::Read { path, .. }
            | HashError::Write { path, .. }
            | HashError::NotUnderBase { path, .. }
            | HashError::NonUtf8Path { path } => Some(path.clone()),
            HashError::Walk(_) => None,
        };
        file.map(|file| Location {
            file: Some(file),
            ..Location::default()
        })
    }
}

/// Compute the SHA-256 hash of the given data.
pub fn sha256(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    hex::encode(result)
}

/// Compute the SHA-256 hash of a file using streaming I/O.
///
/// Uses buffered reads to avoid loading the entire file into memory,
/// which is critical for large build artifacts (firmware images, FPGA bitstreams).
pub fn sha256_file(path: &Path) -> Result<String, HashError> {
    let mut file = fs::File::open(path).map_err(|source| HashError::Open {
        path: path.to_path_buf(),
        source,
    })?;
    let mut hasher = Sha256::new();
    io::copy(&mut file, &mut hasher).map_err(|source| HashError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(hex::encode(hasher.finalize()))
}

/// Compute SHA-256 hash of a file and insert into a BTreeMap.
///
/// The path string is used as the key. Returns an error if the file
/// cannot be read.
pub fn hash_file_into(map: &mut BTreeMap<String, String>, path: &str) -> Result<(), HashError> {
    let hash = sha256_file(std::path::Path::new(path))?;
    map.insert(path.to_string(), hash);
    Ok(())
}

/// Compute SHA-256 hash of a file and insert into a BTreeMap with a relative key.
///
/// The key is computed by stripping the base path from the file path.
/// Path separators are normalized to forward slashes for cross-platform
/// reproducibility (ADR-001 Invariant 6).
pub fn hash_file_relative_into(
    map: &mut BTreeMap<String, String>,
    path: &Path,
    base: &Path,
) -> Result<(), HashError> {
    let hash = sha256_file(path)?;
    let rel = path
        .strip_prefix(base)
        .map_err(|_| HashError::NotUnderBase {
            path: path.to_path_buf(),
            base: base.to_path_buf(),
        })?;
    // Reject non-UTF-8 up-front; `normalize_bundle_path` would otherwise
    // use `to_string_lossy` and silently mangle the key.
    rel.to_str().ok_or_else(|| HashError::NonUtf8Path {
        path: rel.to_path_buf(),
    })?;
    map.insert(normalize_bundle_path(rel), hash);
    Ok(())
}

/// Write SHA256SUMS file for all files in a directory.
///
/// The following files are excluded from the hash list:
/// - `SHA256SUMS` itself (the output file)
/// - `index.json` (metadata layer — contains mutable timestamps)
///
/// Files are sorted for deterministic output.
/// Path separators are normalized to forward slashes for cross-platform
/// reproducibility (ADR-001 Invariant 6).
pub fn write_sha256sums(root: &Path, out_path: &Path) -> Result<(), HashError> {
    let index_path = root.join("index.json");
    let sig_path = root.join("BUNDLE.sig");
    let mut files: Vec<std::path::PathBuf> = Vec::new();
    for entry in walkdir::WalkDir::new(root) {
        let entry = entry.map_err(HashError::Walk)?;
        if entry.file_type().is_file() {
            // Skip metadata-layer files: these are excluded from the content hash
            // to maintain determinism and separation of concerns.
            if entry.path() == out_path {
                continue; // SHA256SUMS itself
            }
            if entry.path() == index_path {
                continue; // index.json (contains timestamps)
            }
            if entry.path() == sig_path {
                continue; // BUNDLE.sig (HMAC signature, written after finalization)
            }
            files.push(entry.path().to_path_buf());
        }
    }
    files.sort(); // Deterministic ordering

    let mut lines = Vec::<String>::new();
    for f in files {
        let rel = f.strip_prefix(root).unwrap_or(&f);
        // Normalize to forward slashes for cross-platform determinism (ADR-001 Invariant 6)
        let rel_path = normalize_bundle_path(rel);
        let hash = sha256_file(&f)?;
        lines.push(format!("{}  {}", hash, rel_path));
    }
    fs::write(out_path, lines.join("\n") + "\n").map_err(|source| HashError::Write {
        path: out_path.to_path_buf(),
        source,
    })?;
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
    fn test_sha256_basic() {
        let hash = sha256(b"hello world");
        assert_eq!(
            hash,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }

    #[test]
    fn test_sha256_empty() {
        let hash = sha256(b"");
        assert_eq!(
            hash,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn test_hash_file_into_nonexistent() {
        let mut map = BTreeMap::new();
        let result = hash_file_into(&mut map, "/nonexistent/file.txt");
        assert!(result.is_err());
    }
}
