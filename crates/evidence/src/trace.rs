//! Traceability types and functions.
//!
//! This module provides types and functions for managing requirements
//! traceability for certification workflows. It handles
//! HLR (High-Level Requirements), LLR (Low-Level Requirements), and
//! Test case linking.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

// Schema is defined canonically in policy.rs; re-export here for backward compatibility.
pub use crate::policy::Schema;

/// Trace document metadata.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct TraceMeta {
    pub document_id: String,
    pub revision: String,
}

// ============================================================================
// High-Level Requirements (HLR)
// ============================================================================

/// HLR TOML file structure.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct HlrFile {
    pub schema: Schema,
    #[allow(dead_code)]
    pub meta: TraceMeta,
    pub requirements: Vec<HlrEntry>,
}

/// A High-Level Requirement entry.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct HlrEntry {
    /// Machine-stable UUID (schema v0.0.2+)
    #[serde(default)]
    pub uid: Option<String>,
    /// Namespace prefix
    #[serde(default)]
    pub ns: Option<String>,
    /// Human-readable ID
    pub id: String,
    /// Requirement title
    pub title: String,
    /// Owner (e.g., "nav-kernel" or "soi") (schema v0.0.3+)
    #[serde(default)]
    pub owner: Option<String>,
    /// Scope ("soi" | "component")
    #[serde(default)]
    #[allow(dead_code)]
    pub scope: Option<String>,
    /// Sort key for deterministic ordering
    #[serde(default)]
    pub sort_key: Option<i64>,
    /// Requirement category
    #[serde(default)]
    #[allow(dead_code)]
    pub category: Option<String>,
    /// Source reference
    #[serde(default)]
    #[allow(dead_code)]
    pub source: Option<String>,
    /// Requirement description
    #[serde(default)]
    pub description: Option<String>,
    /// Rationale for the requirement
    #[serde(default)]
    pub rationale: Option<String>,
    /// Verification methods
    #[serde(default)]
    pub verification_methods: Vec<String>,
}

// ============================================================================
// Low-Level Requirements (LLR)
// ============================================================================

/// LLR TOML file structure.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct LlrFile {
    #[allow(dead_code)]
    pub schema: Schema,
    #[allow(dead_code)]
    pub meta: TraceMeta,
    pub requirements: Vec<LlrEntry>,
}

/// A Low-Level Requirement entry.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct LlrEntry {
    /// Machine-stable UUID (schema v0.0.2+)
    #[serde(default)]
    pub uid: Option<String>,
    /// Namespace prefix
    #[serde(default)]
    pub ns: Option<String>,
    /// Human-readable ID
    pub id: String,
    /// Requirement title
    pub title: String,
    /// Owner (schema v0.0.3+)
    #[serde(default)]
    pub owner: Option<String>,
    /// Sort key for deterministic ordering
    #[serde(default)]
    pub sort_key: Option<i64>,
    /// UIDs of HLRs this LLR traces to
    pub traces_to: Vec<String>,
    /// Source reference
    #[serde(default)]
    #[allow(dead_code)]
    pub source: Option<String>,
    /// Implementation modules
    #[serde(default)]
    #[allow(dead_code)]
    pub modules: Vec<String>,
    /// Whether this is a derived requirement
    #[serde(default)]
    pub derived: bool,
    /// Requirement description
    #[serde(default)]
    pub description: Option<String>,
    /// Rationale for derived requirements
    #[serde(default)]
    pub rationale: Option<String>,
    /// Verification methods
    #[serde(default)]
    pub verification_methods: Vec<String>,
}

// ============================================================================
// Test Cases
// ============================================================================

/// Tests TOML file structure.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct TestsFile {
    #[allow(dead_code)]
    pub schema: Schema,
    #[allow(dead_code)]
    pub meta: TraceMeta,
    pub tests: Vec<TestEntry>,
}

/// A Test case entry.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct TestEntry {
    /// Machine-stable UUID (schema v0.0.2+)
    #[serde(default)]
    pub uid: Option<String>,
    /// Namespace prefix
    #[serde(default)]
    #[allow(dead_code)]
    pub ns: Option<String>,
    /// Human-readable ID
    pub id: String,
    /// Test title
    #[allow(dead_code)]
    pub title: String,
    /// Owner (schema v0.0.3+)
    #[serde(default)]
    pub owner: Option<String>,
    /// Sort key for deterministic ordering
    #[serde(default)]
    pub sort_key: Option<i64>,
    /// UIDs of LLRs this test traces to
    pub traces_to: Vec<String>,
    /// Test description
    #[serde(default)]
    pub description: Option<String>,
    /// Test category
    #[serde(default)]
    #[allow(dead_code)]
    pub category: Option<String>,
    /// Test selector (e.g., test function path)
    #[serde(default)]
    #[allow(dead_code)]
    pub test_selector: Option<String>,
    /// Source reference
    #[serde(default)]
    #[allow(dead_code)]
    pub source: Option<String>,
}

// ============================================================================
// Derived Requirements
// ============================================================================

/// Derived requirements TOML file structure.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct DerivedFile {
    pub schema: Schema,
    pub meta: TraceMeta,
    pub requirements: Vec<DerivedEntry>,
}

/// A Derived Requirement entry.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct DerivedEntry {
    /// Machine-stable UUID
    #[serde(default)]
    pub uid: Option<String>,
    /// Human-readable ID
    pub id: String,
    /// Requirement title
    pub title: String,
    /// Owner
    #[serde(default)]
    pub owner: Option<String>,
    /// Source of derivation (e.g., "implementation", "design")
    #[serde(default)]
    pub source: Option<String>,
    /// Requirement description
    #[serde(default)]
    pub description: Option<String>,
    /// Rationale for the derived requirement
    #[serde(default)]
    pub rationale: Option<String>,
    /// Safety impact level ("none", "low", "medium", "high")
    #[serde(default)]
    pub safety_impact: Option<String>,
    /// Sort key for deterministic ordering
    #[serde(default)]
    pub sort_key: Option<i64>,
}

// ============================================================================
// Parsing Functions
// ============================================================================

/// Parse a TOML file into the given type.
pub fn read_toml<T: for<'de> Deserialize<'de>>(path: &str) -> Result<T> {
    let txt = fs::read_to_string(path).with_context(|| format!("Reading {}", path))?;
    let v = toml::from_str(&txt).with_context(|| format!("Parsing {}", path))?;
    Ok(v)
}

/// Read all trace files from a root directory.
///
/// Returns (HlrFile, LlrFile, TestsFile, Option<DerivedFile>). Missing files
/// are returned with empty requirement lists (derived returns None if absent).
pub fn read_all_trace_files(
    root: &str,
) -> Result<(HlrFile, LlrFile, TestsFile, Option<DerivedFile>)> {
    fn read_or_default<T: for<'de> Deserialize<'de>>(path: &Path, default: T) -> Result<T> {
        if path.exists() {
            read_toml(&path.to_string_lossy())
        } else {
            Ok(default)
        }
    }

    let root_path = Path::new(root);
    let hlr = read_or_default(
        &root_path.join("hlr.toml"),
        HlrFile {
            meta: TraceMeta {
                document_id: "".into(),
                revision: "".into(),
            },
            schema: Schema {
                version: "".into(),
            },
            requirements: vec![],
        },
    )?;
    let llr = read_or_default(
        &root_path.join("llr.toml"),
        LlrFile {
            meta: TraceMeta {
                document_id: "".into(),
                revision: "".into(),
            },
            schema: Schema {
                version: "".into(),
            },
            requirements: vec![],
        },
    )?;
    let tests = read_or_default(
        &root_path.join("tests.toml"),
        TestsFile {
            meta: TraceMeta {
                document_id: "".into(),
                revision: "".into(),
            },
            schema: Schema {
                version: "".into(),
            },
            tests: vec![],
        },
    )?;

    let derived_path = root_path.join("derived.toml");
    let derived = if derived_path.exists() {
        Some(read_toml::<DerivedFile>(&derived_path.to_string_lossy())?)
    } else {
        None
    };

    Ok((hlr, llr, tests, derived))
}

// ============================================================================
// UUID Backfill
// ============================================================================

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
        let mut hlr: HlrFile = read_toml(&hlr_path.to_string_lossy())?;
        let n = assign_missing_uuids_hlr(&mut hlr.requirements);
        if n > 0 {
            let content = toml::to_string_pretty(&hlr)
                .with_context(|| "serializing hlr.toml")?;
            fs::write(&hlr_path, content)
                .with_context(|| format!("writing {}", hlr_path.display()))?;
            total += n;
        }
    }

    // LLR
    let llr_path = root_path.join("llr.toml");
    if llr_path.exists() {
        let mut llr: LlrFile = read_toml(&llr_path.to_string_lossy())?;
        let n = assign_missing_uuids_llr(&mut llr.requirements);
        if n > 0 {
            let content = toml::to_string_pretty(&llr)
                .with_context(|| "serializing llr.toml")?;
            fs::write(&llr_path, content)
                .with_context(|| format!("writing {}", llr_path.display()))?;
            total += n;
        }
    }

    // Tests
    let tests_path = root_path.join("tests.toml");
    if tests_path.exists() {
        let mut tests: TestsFile = read_toml(&tests_path.to_string_lossy())?;
        let n = assign_missing_uuids_test(&mut tests.tests);
        if n > 0 {
            let content = toml::to_string_pretty(&tests)
                .with_context(|| "serializing tests.toml")?;
            fs::write(&tests_path, content)
                .with_context(|| format!("writing {}", tests_path.display()))?;
            total += n;
        }
    }

    // Derived
    let derived_path = root_path.join("derived.toml");
    if derived_path.exists() {
        let mut derived: DerivedFile = read_toml(&derived_path.to_string_lossy())?;
        let n = assign_missing_uuids_derived(&mut derived.requirements);
        if n > 0 {
            let content = toml::to_string_pretty(&derived)
                .with_context(|| "serializing derived.toml")?;
            fs::write(&derived_path, content)
                .with_context(|| format!("writing {}", derived_path.display()))?;
            total += n;
        }
    }

    Ok(total)
}

// ============================================================================
// Validation
// ============================================================================

/// Validate trace links between HLRs, LLRs, and Tests.
///
/// Checks:
/// - All items have UIDs and owners
/// - No duplicate UIDs
/// - All links point to valid UIDs
/// - Ownership rules are respected
/// - Derived LLRs have rationale
/// - All items have verification methods
pub fn validate_trace_links(
    hlrs: &[HlrEntry],
    llrs: &[LlrEntry],
    tests: &[TestEntry],
) -> Result<()> {
    let mut errors: Vec<String> = Vec::new();

    // Index: uid -> (kind, owner, id)
    let mut uid_index: BTreeMap<String, (String, String, String)> = BTreeMap::new();
    // Index: (kind, owner, id) -> uid (to check item uniqueness)
    let mut item_index: BTreeMap<(String, String, String), String> = BTreeMap::new();

    let mut register =
        |uid: &Option<String>, owner: &Option<String>, id: &String, kind: &str| {
            let o = if let Some(ow) = owner {
                ow.clone()
            } else {
                errors.push(format!("[{}:{}] missing 'owner'", kind, id));
                return; // Strict fail
            };

            let u = match uid {
                Some(u) => {
                    if uuid::Uuid::parse_str(u).is_err() {
                        errors.push(format!("[{}:{}] invalid UID format '{}'", kind, id, u));
                        return;
                    }
                    u.clone()
                }
                None => {
                    errors.push(format!("[{}:{}] missing UID", kind, id));
                    return;
                }
            };

            if let Some((prev_kind, prev_owner, prev_id)) = uid_index.get(&u) {
                errors.push(format!(
                    "Duplicate UID {}: used by [{}({}):{}] and [{}({}):{}]",
                    u, prev_kind, prev_owner, prev_id, kind, o, id
                ));
            } else {
                uid_index.insert(u.clone(), (kind.to_string(), o.clone(), id.clone()));
            }

            let key = (kind.to_string(), o.clone(), id.clone());
            if let Some(prev_uid) = item_index.get(&key) {
                errors.push(format!(
                    "Duplicate Item '{}({}):{}': used by {} and {}",
                    kind, o, id, prev_uid, u
                ));
            } else {
                item_index.insert(key, u);
            }
        };

    for r in hlrs {
        register(&r.uid, &r.owner, &r.id, "HLR");
    }
    for r in llrs {
        register(&r.uid, &r.owner, &r.id, "LLR");
    }
    for t in tests {
        register(&t.uid, &t.owner, &t.id, "TEST");
    }

    if !errors.is_empty() {
        for e in &errors {
            eprintln!("  VALIDATION ERROR: {}", e);
        }
        bail!(
            "Validation failed with {} errors (fix before linking check)",
            errors.len()
        );
    }

    // Link Validation
    let check_link = |source_kind: &str,
                      source_id: &str,
                      source_owner: &Option<String>,
                      link: &str,
                      expected_target_kind: &str|
     -> Option<String> {
        // 1. Must be UUID
        if uuid::Uuid::parse_str(link).is_err() {
            return Some(format!(
                "Link '{}' in {} is not a UUID",
                link, source_id
            ));
        }

        // 2. Must Exist
        let (target_kind, target_owner, target_id) = match uid_index.get(link) {
            Some(t) => t,
            None => {
                return Some(format!(
                    "Link '{}' in {} not found (dangling ref)",
                    link, source_id
                ))
            }
        };

        // 3. Kind Check
        if target_kind != expected_target_kind {
            return Some(format!(
                "Link '{}' in {} points to {} but expected {}",
                link, source_id, target_kind, expected_target_kind
            ));
        }

        // 4. Ownership Logic
        let s_owner = source_owner.as_ref().map(|s| s.as_str()).unwrap_or("UNKNOWN");
        let t_owner = target_owner.as_str();

        match (source_kind, expected_target_kind) {
            ("LLR", "HLR") => {
                // Allowed: same owner OR target is "soi"/"project"
                if s_owner == t_owner || t_owner == "soi" || t_owner == "project" {
                    // OK
                } else {
                    return Some(format!(
                        "Ownership violation: LLR({}:{}) -> HLR({}:{}). Cross-crate link forbidden.",
                        s_owner, source_id, t_owner, target_id
                    ));
                }
            }
            ("TEST", "LLR") => {
                // Allowed: strictly same owner
                if s_owner != t_owner {
                    return Some(format!(
                        "Ownership violation: TEST({}:{}) -> LLR({}:{}). Must be same crate.",
                        s_owner, source_id, t_owner, target_id
                    ));
                }
            }
            _ => { /* Checks not implemented for other pairings */ }
        }

        None
    };

    // Strict Policy Checks

    // HLR Policy
    for r in hlrs {
        if r.verification_methods.is_empty() {
            errors.push(format!("HLR missing verification_methods: {}", r.id));
        }
    }

    for r in llrs {
        // LLR Policy: Derived vs Traced
        if r.traces_to.is_empty() {
            if !r.derived {
                errors.push(format!(
                    "LLR {} has no parent links. Must be marked 'derived = true'",
                    r.id
                ));
            } else if r.rationale.as_ref().map(|s| s.is_empty()).unwrap_or(true) {
                errors.push(format!("Derived LLR {} missing 'rationale'", r.id));
            }
        } else if r.derived {
            errors.push(format!(
                "LLR {} is marked derived but has trace links. Contradiction.",
                r.id
            ));
        }

        if r.verification_methods.is_empty() {
            errors.push(format!("LLR missing verification_methods: {}", r.id));
        }

        let mut seen_links = BTreeSet::new();
        for link in &r.traces_to {
            if !seen_links.insert(link) {
                errors.push(format!("LLR {} has duplicate trace link '{}'", r.id, link));
            }
            if let Some(e) = check_link("LLR", &r.id, &r.owner, link, "HLR") {
                errors.push(e);
            }
        }
    }
    for t in tests {
        let mut seen_links = BTreeSet::new();
        for link in &t.traces_to {
            if !seen_links.insert(link) {
                errors.push(format!("TEST {} has duplicate trace link '{}'", t.id, link));
            }
            if let Some(e) = check_link("TEST", &t.id, &t.owner, link, "LLR") {
                errors.push(e);
            }
        }
    }

    // Orphan test detection: tests with empty traces_to list
    let orphan_tests: Vec<&TestEntry> = tests.iter().filter(|t| t.traces_to.is_empty()).collect();
    if !orphan_tests.is_empty() {
        for t in &orphan_tests {
            eprintln!(
                "  WARNING: Orphan test '{}' is not linked to any LLR",
                t.id
            );
        }
        eprintln!(
            "  WARNING: {} orphan test(s) found (tests with no LLR link)",
            orphan_tests.len()
        );
    }

    if !errors.is_empty() {
        for e in &errors {
            eprintln!("  LINK ERROR: {}", e);
        }
        bail!("Trace link validation failed with {} errors", errors.len());
    }

    Ok(())
}

// ============================================================================
// Traceability Matrix Generation
// ============================================================================

/// Generate a Markdown traceability matrix document.
///
/// The output is deterministic (no timestamps) for reproducibility.
pub fn generate_traceability_matrix(
    hlr: &HlrFile,
    llr: &LlrFile,
    tests: &TestsFile,
    doc_id: &str,
) -> Result<String> {
    let mut s = String::new();

    // Header - NO TIMESTAMPS for determinism
    s.push_str("<!-- GENERATED FILE. DO NOT EDIT.\n");
    s.push_str("     Source of truth: cert/trace/*.toml\n");
    s.push_str("     Regenerate: cargo xtask trace\n");
    s.push_str("-->\n\n");
    s.push_str("# Traceability Matrix\n\n");
    s.push_str("<!-- Source: cert/trace roots (see project.toml trace.roots) -->\n");
    s.push_str(&format!("**Document ID:** {}\n\n", doc_id));

    // Sort by sort_key, then by ID for determinism
    let mut hlrs = hlr.requirements.clone();
    hlrs.sort_by(|a, b| {
        a.sort_key
            .unwrap_or(0)
            .cmp(&b.sort_key.unwrap_or(0))
            .then_with(|| a.id.cmp(&b.id))
    });

    let mut llrs = llr.requirements.clone();
    llrs.sort_by(|a, b| {
        a.sort_key
            .unwrap_or(0)
            .cmp(&b.sort_key.unwrap_or(0))
            .then_with(|| a.id.cmp(&b.id))
    });

    let mut ts = tests.tests.clone();
    ts.sort_by(|a, b| {
        a.sort_key
            .unwrap_or(0)
            .cmp(&b.sort_key.unwrap_or(0))
            .then_with(|| a.id.cmp(&b.id))
    });

    // HLR -> LLR table
    s.push_str("## HLR to LLR Traceability\n\n");
    s.push_str("| HLR ID | HLR Title | LLR IDs |\n");
    s.push_str("|--------|-----------|--------|\n");

    for h in &hlrs {
        // Find LLRs that trace to this HLR (Strict UUID match)
        let mut linked: Vec<&str> = llrs
            .iter()
            .filter(|l| l.traces_to.iter().any(|x| Some(x) == h.uid.as_ref()))
            .map(|l| l.id.as_str())
            .collect();
        linked.sort(); // Deterministic order

        let llr_cell = if linked.is_empty() {
            "*(none)*".to_string()
        } else {
            linked.join(", ")
        };
        let display_id = if let Some(ns) = &h.ns {
            format!("{}:{}", ns, h.id)
        } else {
            h.id.clone()
        };
        s.push_str(&format!("| {} | {} | {} |\n", display_id, h.title, llr_cell));
    }

    // LLR -> TEST table
    s.push_str("\n## LLR to Test Traceability\n\n");
    s.push_str("| LLR ID | LLR Title | Test IDs |\n");
    s.push_str("|--------|-----------|----------|\n");

    for l in &llrs {
        let mut linked: Vec<&str> = ts
            .iter()
            .filter(|t| t.traces_to.iter().any(|x| Some(x) == l.uid.as_ref()))
            .map(|t| t.id.as_str())
            .collect();
        linked.sort();

        let test_cell = if linked.is_empty() {
            "*(none)*".to_string()
        } else {
            linked.join(", ")
        };
        let display_id = if let Some(ns) = &l.ns {
            format!("{}:{}", ns, l.id)
        } else {
            l.id.clone()
        };
        s.push_str(&format!("| {} | {} | {} |\n", display_id, l.title, test_cell));
    }

    // ================================================================
    // Reverse Trace Table: Test -> LLR -> HLR
    // ================================================================

    // Build lookup: LLR UID -> list of HLR IDs it traces to
    let mut llr_uid_to_hlr_ids: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for l in &llrs {
        if let Some(ref uid) = l.uid {
            let mut parent_ids: Vec<String> = Vec::new();
            for link in &l.traces_to {
                // Find the HLR with this UID
                if let Some(h) = hlrs.iter().find(|h| h.uid.as_ref() == Some(link)) {
                    let display_id = if let Some(ns) = &h.ns {
                        format!("{}:{}", ns, h.id)
                    } else {
                        h.id.clone()
                    };
                    parent_ids.push(display_id);
                }
            }
            parent_ids.sort();
            llr_uid_to_hlr_ids.insert(uid.clone(), parent_ids);
        }
    }

    s.push_str("\n## Reverse Trace: Test to LLR to HLR\n\n");
    s.push_str("| Test ID | LLR IDs | HLR IDs |\n");
    s.push_str("|---------|---------|--------|\n");

    for t in &ts {
        // Find LLR IDs this test traces to
        let mut llr_ids: Vec<String> = Vec::new();
        let mut hlr_ids_set: BTreeSet<String> = BTreeSet::new();

        for link in &t.traces_to {
            if let Some(l) = llrs.iter().find(|l| l.uid.as_ref() == Some(link)) {
                let display_id = if let Some(ns) = &l.ns {
                    format!("{}:{}", ns, l.id)
                } else {
                    l.id.clone()
                };
                llr_ids.push(display_id);

                // Roll up to HLR via LLR UID
                if let Some(ref uid) = l.uid {
                    if let Some(parent_hlrs) = llr_uid_to_hlr_ids.get(uid) {
                        for h_id in parent_hlrs {
                            hlr_ids_set.insert(h_id.clone());
                        }
                    }
                }
            }
        }
        llr_ids.sort();

        let llr_cell = if llr_ids.is_empty() {
            "*(none)*".to_string()
        } else {
            llr_ids.join(", ")
        };
        let hlr_cell = if hlr_ids_set.is_empty() {
            "*(none)*".to_string()
        } else {
            hlr_ids_set.into_iter().collect::<Vec<_>>().join(", ")
        };

        let display_id = if let Some(ns) = &t.ns {
            format!("{}:{}", ns, t.id)
        } else {
            t.id.clone()
        };
        s.push_str(&format!("| {} | {} | {} |\n", display_id, llr_cell, hlr_cell));
    }

    // ================================================================
    // End-to-End HLR -> Test Roll-Up Table
    // ================================================================

    s.push_str("\n## End-to-End: HLR to Test Roll-Up\n\n");
    s.push_str("| HLR ID | HLR Title | Test IDs (via LLR) |\n");
    s.push_str("|--------|-----------|--------------------|\n");

    for h in &hlrs {
        // Find all LLRs that trace to this HLR
        let child_llrs: Vec<&LlrEntry> = llrs
            .iter()
            .filter(|l| l.traces_to.iter().any(|x| Some(x) == h.uid.as_ref()))
            .collect();

        // Find all tests that trace to any of those LLRs
        let mut test_ids: BTreeSet<String> = BTreeSet::new();
        for l in &child_llrs {
            for t in &ts {
                if t.traces_to.iter().any(|x| l.uid.as_ref() == Some(x)) {
                    let display_id = if let Some(ns) = &t.ns {
                        format!("{}:{}", ns, t.id)
                    } else {
                        t.id.clone()
                    };
                    test_ids.insert(display_id);
                }
            }
        }

        let test_cell = if test_ids.is_empty() {
            "*(none)*".to_string()
        } else {
            test_ids.into_iter().collect::<Vec<_>>().join(", ")
        };
        let display_id = if let Some(ns) = &h.ns {
            format!("{}:{}", ns, h.id)
        } else {
            h.id.clone()
        };
        s.push_str(&format!("| {} | {} | {} |\n", display_id, h.title, test_cell));
    }

    // ================================================================
    // Orphan Test Detection
    // ================================================================

    let orphan_tests: Vec<&TestEntry> = ts.iter().filter(|t| t.traces_to.is_empty()).collect();

    // ================================================================
    // Coverage Summary
    // ================================================================

    let hlr_without_llr: usize = hlrs
        .iter()
        .filter(|h| {
            !llrs
                .iter()
                .any(|l| l.traces_to.iter().any(|x| Some(x) == h.uid.as_ref()))
        })
        .count();
    let llr_without_test: usize = llrs
        .iter()
        .filter(|l| {
            !ts.iter()
                .any(|t| t.traces_to.iter().any(|x| Some(x) == l.uid.as_ref()))
        })
        .count();

    s.push_str("\n## Coverage Summary\n\n");
    s.push_str(&format!("- **HLR count:** {}\n", hlrs.len()));
    s.push_str(&format!("- **LLR count:** {}\n", llrs.len()));
    s.push_str(&format!("- **Test count:** {}\n", ts.len()));
    s.push_str(&format!("- **HLR without LLR:** {}\n", hlr_without_llr));
    s.push_str(&format!("- **LLR without Test:** {}\n", llr_without_test));
    s.push_str(&format!("- **Orphan tests (no LLR link):** {}\n", orphan_tests.len()));
    s.push('\n');

    // Gap report
    if hlr_without_llr > 0 || llr_without_test > 0 || !orphan_tests.is_empty() {
        s.push_str("## Gaps\n\n");

        if hlr_without_llr > 0 {
            s.push_str("### HLRs without LLR coverage\n\n");
            for h in &hlrs {
                if !llrs
                    .iter()
                    .any(|l| l.traces_to.iter().any(|x| Some(x) == h.uid.as_ref()))
                {
                    s.push_str(&format!("- {} ({})\n", h.id, h.title));
                }
            }
            s.push('\n');
        }

        if llr_without_test > 0 {
            s.push_str("### LLRs without Test coverage\n\n");
            for l in &llrs {
                if !ts
                    .iter()
                    .any(|t| t.traces_to.iter().any(|x| Some(x) == l.uid.as_ref()))
                {
                    s.push_str(&format!("- {} ({})\n", l.id, l.title));
                }
            }
            s.push('\n');
        }

        if !orphan_tests.is_empty() {
            s.push_str("### Orphan Tests (no LLR link)\n\n");
            for t in &orphan_tests {
                let display_id = if let Some(ns) = &t.ns {
                    format!("{}:{}", ns, t.id)
                } else {
                    t.id.clone()
                };
                s.push_str(&format!("- {} ({})\n", display_id, t.title));
            }
            s.push('\n');
        }
    }

    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hlr_entry_fields() {
        let hlr = HlrEntry {
            uid: Some("test-uuid".to_string()),
            ns: None,
            id: "HLR-001".to_string(),
            title: "Test Requirement".to_string(),
            owner: Some("nav-kernel".to_string()),
            scope: None,
            sort_key: Some(1),
            category: None,
            source: None,
            description: Some("A test requirement".to_string()),
            rationale: Some("Because we need it".to_string()),
            verification_methods: vec!["test".to_string()],
        };
        assert_eq!(hlr.id, "HLR-001");
        assert!(hlr.uid.is_some());
        assert_eq!(hlr.description.as_deref(), Some("A test requirement"));
        assert_eq!(hlr.rationale.as_deref(), Some("Because we need it"));
    }

    #[test]
    fn test_llr_entry_fields() {
        let llr = LlrEntry {
            uid: Some("test-uuid".to_string()),
            ns: None,
            id: "LLR-001".to_string(),
            title: "Test LLR".to_string(),
            owner: Some("nav-kernel".to_string()),
            sort_key: None,
            traces_to: vec!["parent-uuid".to_string()],
            source: None,
            modules: vec![],
            derived: false,
            description: Some("An LLR description".to_string()),
            rationale: None,
            verification_methods: vec!["test".to_string()],
        };
        assert_eq!(llr.id, "LLR-001");
        assert!(!llr.derived);
        assert_eq!(llr.description.as_deref(), Some("An LLR description"));
    }

    #[test]
    fn test_assign_missing_uuids_hlr() {
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
        assert!(entries[0].uid.is_some());
        // Verify it's a valid UUID
        assert!(uuid::Uuid::parse_str(entries[0].uid.as_ref().unwrap()).is_ok());
        // The existing one should be untouched
        assert_eq!(entries[1].uid.as_deref(), Some("existing-uuid"));
    }

    #[test]
    fn test_assign_missing_uuids_derived() {
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
        assert!(uuid::Uuid::parse_str(entries[0].uid.as_ref().unwrap()).is_ok());
    }
}
