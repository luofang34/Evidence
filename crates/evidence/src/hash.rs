//! Cryptographic hashing utilities.
//!
//! This module provides consistent hashing functions for
//! computing digests of files and data.

use sha2::{Digest, Sha256};

/// Compute the SHA-256 hash of the given data.
pub fn sha256(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    hex::encode(result)
}

/// Compute the SHA-256 hash of a file.
pub fn sha256_file(path: &std::path::Path) -> anyhow::Result<String> {
    let data = std::fs::read(path)?;
    Ok(sha256(&data))
}
