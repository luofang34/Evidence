//! Cryptographic hashing utilities.
//!
//! This module provides consistent hashing functions for
//! computing digests of files and data. Uses BTreeMap for
//! deterministic ordering in hash collections.

use anyhow::{Context, Result};
use log;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

/// Compute the SHA-256 hash of the given data.
pub fn sha256(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    hex::encode(result)
}

/// Compute the SHA-256 hash of a file.
pub fn sha256_file(path: &Path) -> Result<String> {
    let data = fs::read(path).with_context(|| format!("Reading {:?}", path))?;
    Ok(sha256(&data))
}

/// Compute SHA-256 hash of a file and insert into a BTreeMap.
///
/// The path string is used as the key. Returns an error if the file
/// cannot be read.
pub fn hash_file_into(map: &mut BTreeMap<String, String>, path: &str) -> Result<()> {
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
) -> Result<()> {
    let hash = sha256_file(path)?;
    let rel = path
        .strip_prefix(base)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/");
    map.insert(rel, hash);
    Ok(())
}

/// Collect hashes for a list of files into a BTreeMap.
///
/// When `strict` is false, files that cannot be read are skipped with a
/// warning. When `strict` is true (cert/record profiles), any file that
/// cannot be read causes a hard error in strict mode.
///
/// Uses BTreeMap for deterministic ordering.
pub fn collect_input_hashes(files: &[String], strict: bool) -> Result<BTreeMap<String, String>> {
    let mut hashes = BTreeMap::new();
    for file in files {
        if let Err(e) = hash_file_into(&mut hashes, file) {
            if strict {
                return Err(e.context(format!(
                    "strict mode: cannot hash input file '{}' (cert/record profile requires all inputs readable)",
                    file
                )));
            }
            log::warn!("could not hash {}: {}", file, e);
        }
    }
    Ok(hashes)
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
pub fn write_sha256sums(root: &Path, out_path: &Path) -> Result<()> {
    let index_path = root.join("index.json");
    let sig_path = root.join("BUNDLE.sig");
    let mut files: Vec<std::path::PathBuf> = Vec::new();
    for entry in walkdir::WalkDir::new(root) {
        let entry = entry?;
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
        let rel_path = rel.to_string_lossy().replace('\\', "/");
        let hash = sha256_file(&f)?;
        lines.push(format!("{}  {}", hash, rel_path));
    }
    fs::write(out_path, lines.join("\n") + "\n")?;
    Ok(())
}

#[cfg(test)]
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
    fn test_collect_input_hashes_empty() {
        let hashes = collect_input_hashes(&[], false).unwrap();
        assert!(hashes.is_empty());
    }

    #[test]
    fn test_collect_input_hashes_strict_missing_file() {
        let result = collect_input_hashes(&["/nonexistent/file.txt".to_string()], true);
        assert!(result.is_err());
    }

    #[test]
    fn test_collect_input_hashes_lenient_missing_file() {
        let hashes = collect_input_hashes(&["/nonexistent/file.txt".to_string()], false).unwrap();
        assert!(hashes.is_empty());
    }

    #[test]
    fn test_hash_file_into_nonexistent() {
        let mut map = BTreeMap::new();
        let result = hash_file_into(&mut map, "/nonexistent/file.txt");
        assert!(result.is_err());
    }
}
