//! UUID assignment + on-disk backfill for trace entries.
//!
//! Every entry kind (HLR / LLR / Test / Derived) declares `uid: Option<String>`
//! with `#[serde(default)]` so legacy TOML without `uid` fields parses.
//! `assign_missing_uuids_*` fills in a fresh v4 UUID for every `None`;
//! `backfill_uuids` reads each trace file, runs the appropriate
//! assigner, and writes the file back with `toml::to_string_pretty`
//! only if any UUIDs were actually added.

use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;

use super::entries::{
    DerivedEntry, DerivedFile, HlrEntry, HlrFile, LlrEntry, LlrFile, TestEntry, TestsFile,
};
use super::read::{TraceReadError, read_toml};

/// Errors returned by [`backfill_uuids`].
#[derive(Debug, Error)]
pub enum BackfillError {
    /// Reading / parsing one of the trace TOML files failed.
    #[error(transparent)]
    Read(#[from] TraceReadError),
    /// Writing the updated TOML file back to disk failed.
    #[error("writing {path}")]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// `toml::to_string_pretty` round-trip serialization failed.
    /// Boxed to keep the enum under clippy's `result_large_err`
    /// threshold on Windows.
    #[error("serializing {filename}")]
    Serialize {
        filename: &'static str,
        #[source]
        source: Box<toml::ser::Error>,
    },
}

/// Assign UUIDs to any HLR entries that are missing them.
/// Returns the number of UUIDs assigned.
pub fn assign_missing_uuids_hlr(entries: &mut [HlrEntry]) -> usize {
    let mut count = 0;
    for entry in entries.iter_mut() {
        if entry.uid.is_none() {
            entry.uid = Some(uuid::Uuid::new_v4().to_string());
            count += 1;
        }
    }
    count
}

/// Assign UUIDs to any LLR entries that are missing them.
/// Returns the number of UUIDs assigned.
pub fn assign_missing_uuids_llr(entries: &mut [LlrEntry]) -> usize {
    let mut count = 0;
    for entry in entries.iter_mut() {
        if entry.uid.is_none() {
            entry.uid = Some(uuid::Uuid::new_v4().to_string());
            count += 1;
        }
    }
    count
}

/// Assign UUIDs to any Test entries that are missing them.
/// Returns the number of UUIDs assigned.
pub fn assign_missing_uuids_test(entries: &mut [TestEntry]) -> usize {
    let mut count = 0;
    for entry in entries.iter_mut() {
        if entry.uid.is_none() {
            entry.uid = Some(uuid::Uuid::new_v4().to_string());
            count += 1;
        }
    }
    count
}

/// Assign UUIDs to any Derived entries that are missing them.
/// Returns the number of UUIDs assigned.
pub fn assign_missing_uuids_derived(entries: &mut [DerivedEntry]) -> usize {
    let mut count = 0;
    for entry in entries.iter_mut() {
        if entry.uid.is_none() {
            entry.uid = Some(uuid::Uuid::new_v4().to_string());
            count += 1;
        }
    }
    count
}

/// Read trace files from a directory, assign missing UUIDs, and write back.
/// Returns total number of UUIDs assigned.
pub fn backfill_uuids(trace_root: &str) -> Result<usize, BackfillError> {
    let root_path = Path::new(trace_root);
    let mut total = 0;

    // HLR
    let hlr_path = root_path.join("hlr.toml");
    if hlr_path.exists() {
        let mut hlr: HlrFile = read_toml(&hlr_path)?;
        let n = assign_missing_uuids_hlr(&mut hlr.requirements);
        if n > 0 {
            let content =
                toml::to_string_pretty(&hlr).map_err(|source| BackfillError::Serialize {
                    filename: "hlr.toml",
                    source: Box::new(source),
                })?;
            fs::write(&hlr_path, content).map_err(|source| BackfillError::Write {
                path: hlr_path.clone(),
                source,
            })?;
            total += n;
        }
    }

    // LLR
    let llr_path = root_path.join("llr.toml");
    if llr_path.exists() {
        let mut llr: LlrFile = read_toml(&llr_path)?;
        let n = assign_missing_uuids_llr(&mut llr.requirements);
        if n > 0 {
            let content =
                toml::to_string_pretty(&llr).map_err(|source| BackfillError::Serialize {
                    filename: "llr.toml",
                    source: Box::new(source),
                })?;
            fs::write(&llr_path, content).map_err(|source| BackfillError::Write {
                path: llr_path.clone(),
                source,
            })?;
            total += n;
        }
    }

    // Tests
    let tests_path = root_path.join("tests.toml");
    if tests_path.exists() {
        let mut tests: TestsFile = read_toml(&tests_path)?;
        let n = assign_missing_uuids_test(&mut tests.tests);
        if n > 0 {
            let content =
                toml::to_string_pretty(&tests).map_err(|source| BackfillError::Serialize {
                    filename: "tests.toml",
                    source: Box::new(source),
                })?;
            fs::write(&tests_path, content).map_err(|source| BackfillError::Write {
                path: tests_path.clone(),
                source,
            })?;
            total += n;
        }
    }

    // Derived
    let derived_path = root_path.join("derived.toml");
    if derived_path.exists() {
        let mut derived: DerivedFile = read_toml(&derived_path)?;
        let n = assign_missing_uuids_derived(&mut derived.requirements);
        if n > 0 {
            let content =
                toml::to_string_pretty(&derived).map_err(|source| BackfillError::Serialize {
                    filename: "derived.toml",
                    source: Box::new(source),
                })?;
            fs::write(&derived_path, content).map_err(|source| BackfillError::Write {
                path: derived_path.clone(),
                source,
            })?;
            total += n;
        }
    }

    Ok(total)
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
    fn test_assign_missing_uuids_hlr() -> anyhow::Result<()> {
        let mut entries = vec![
            HlrEntry {
                uid: None,
                ns: None,
                id: "HLR-001".to_string(),
                title: "Needs UUID".to_string(),
                owner: None,
                scope: None,
                sort_key: None,
                category: None,
                source: None,
                description: None,
                rationale: None,
                verification_methods: vec![],
            },
            HlrEntry {
                uid: Some("existing-uuid".to_string()),
                ns: None,
                id: "HLR-002".to_string(),
                title: "Has UUID".to_string(),
                owner: None,
                scope: None,
                sort_key: None,
                category: None,
                source: None,
                description: None,
                rationale: None,
                verification_methods: vec![],
            },
        ];

        let count = assign_missing_uuids_hlr(&mut entries);
        assert_eq!(count, 1);
        let assigned_uid = entries[0]
            .uid
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("expected uid to be assigned"))?;
        assert!(uuid::Uuid::parse_str(assigned_uid).is_ok());
        // The existing one should be untouched.
        assert_eq!(entries[1].uid.as_deref(), Some("existing-uuid"));
        Ok(())
    }

    #[test]
    fn test_assign_missing_uuids_derived() -> anyhow::Result<()> {
        let mut entries = vec![DerivedEntry {
            uid: None,
            id: "DER-001".to_string(),
            title: "Derived req".to_string(),
            owner: None,
            source: None,
            description: None,
            rationale: None,
            safety_impact: None,
            sort_key: None,
        }];

        let count = assign_missing_uuids_derived(&mut entries);
        assert_eq!(count, 1);
        let assigned_uid = entries[0]
            .uid
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("expected uid to be assigned"))?;
        assert!(uuid::Uuid::parse_str(assigned_uid).is_ok());
        Ok(())
    }
}
