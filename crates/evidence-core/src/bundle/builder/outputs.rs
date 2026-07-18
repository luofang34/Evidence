//! Output-hashing methods for [`EvidenceBuilder`](super::EvidenceBuilder).
//!
//! A child module of `builder`, so these methods reach
//! `EvidenceBuilder`'s private `outputs` / `bundle_dir` fields directly.

use std::path::Path;

use super::EvidenceBuilder;
use crate::bundle::BuilderError;
use crate::hash::hash_file_relative_into;

impl EvidenceBuilder {
    /// Hash a file with relative path and add to outputs.
    pub fn hash_output(&mut self, path: &Path) -> Result<(), BuilderError> {
        Ok(hash_file_relative_into(
            &mut self.outputs,
            path,
            &self.bundle_dir,
        )?)
    }

    /// Hash a build artifact that lives outside the bundle (under
    /// `target/`) and record it under a canonical `key`. Used by the
    /// output-inventory phase, where `key` is the artifact's path
    /// relative to cargo's `target_directory` and `file_path` is absolute.
    pub fn add_output(&mut self, key: String, file_path: &Path) -> Result<(), BuilderError> {
        let hash = crate::hash::sha256_file(file_path)?;
        self.outputs.insert(key, hash);
        Ok(())
    }
}
