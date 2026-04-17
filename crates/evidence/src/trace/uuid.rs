//! UUID assignment + on-disk backfill for trace entries.
//!
//! Every entry kind (HLR / LLR / Test / Derived) declares `uid: Option<String>`
//! with `#[serde(default)]` so legacy TOML without `uid` fields parses.
//! `assign_missing_uuids_*` fills in a fresh v4 UUID for every `None`;
//! `backfill_uuids` reads each trace file, runs the appropriate
//! assigner, and writes the file back with `toml::to_string_pretty`
//! only if any UUIDs were actually added.

use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

use super::entries::{
    DerivedEntry, DerivedFile, HlrEntry, HlrFile, LlrEntry, LlrFile, TestEntry, TestsFile,
};
use super::read::read_toml;

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
pub fn backfill_uuids(trace_root: &str) -> Result<usize> {
    let root_path = Path::new(trace_root);
    let mut total = 0;

    // HLR
    let hlr_path = root_path.join("hlr.toml");
    if hlr_path.exists() {
        let mut hlr: HlrFile = read_toml(&hlr_path)?;
        let n = assign_missing_uuids_hlr(&mut hlr.requirements);
        if n > 0 {
            let content = toml::to_string_pretty(&hlr).with_context(|| "serializing hlr.toml")?;
            fs::write(&hlr_path, content)
                .with_context(|| format!("writing {}", hlr_path.display()))?;
            total += n;
        }
    }

    // LLR
    let llr_path = root_path.join("llr.toml");
    if llr_path.exists() {
        let mut llr: LlrFile = read_toml(&llr_path)?;
        let n = assign_missing_uuids_llr(&mut llr.requirements);
        if n > 0 {
            let content = toml::to_string_pretty(&llr).with_context(|| "serializing llr.toml")?;
            fs::write(&llr_path, content)
                .with_context(|| format!("writing {}", llr_path.display()))?;
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
                toml::to_string_pretty(&tests).with_context(|| "serializing tests.toml")?;
            fs::write(&tests_path, content)
                .with_context(|| format!("writing {}", tests_path.display()))?;
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
                toml::to_string_pretty(&derived).with_context(|| "serializing derived.toml")?;
            fs::write(&derived_path, content)
                .with_context(|| format!("writing {}", derived_path.display()))?;
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
