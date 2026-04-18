//! Data types for trace files: HLR, LLR, Test, Derived, shared metadata.
//!
//! Each of the four traceability files on disk (`hlr.toml`, `llr.toml`,
//! `tests.toml`, optional `derived.toml`) deserializes into a `*File`
//! wrapper whose single list field carries `*Entry` items. The items
//! share enough cross-cutting structure (uid, owner, sort_key) that
//! `validate_trace_links` can treat them uniformly via a `register`
//! closure without a trait object.

use serde::{Deserialize, Serialize};

// Schema is defined canonically in policy.rs; the public API of this
// module re-exports it for backward compatibility.
pub use crate::policy::Schema;

/// Trace document metadata (shared by every `*File` wrapper).
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct TraceMeta {
    /// Stable identifier for the document (e.g. `"NAV-HLR-001"`).
    pub document_id: String,
    /// Revision label carried in the header (e.g. `"1.0"`).
    pub revision: String,
}

// ============================================================================
// High-Level Requirements (HLR)
// ============================================================================

/// HLR TOML file structure.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct HlrFile {
    /// Schema version label for the file.
    pub schema: Schema,
    /// Document-level metadata (id + revision).
    pub meta: TraceMeta,
    /// Requirement entries in this file.
    pub requirements: Vec<HlrEntry>,
}

/// A High-Level Requirement entry.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct HlrEntry {
    /// Machine-stable UUID.
    #[serde(default)]
    pub uid: Option<String>,
    /// Namespace prefix.
    #[serde(default)]
    pub ns: Option<String>,
    /// Human-readable ID.
    pub id: String,
    /// Requirement title.
    pub title: String,
    /// Owner (e.g., `nav-kernel` or `soi`).
    #[serde(default)]
    pub owner: Option<String>,
    /// Scope (`soi` | `component`).
    #[serde(default)]
    pub scope: Option<String>,
    /// Sort key for deterministic ordering.
    #[serde(default)]
    pub sort_key: Option<i64>,
    /// Requirement category.
    #[serde(default)]
    pub category: Option<String>,
    /// Source reference.
    #[serde(default)]
    pub source: Option<String>,
    /// Requirement description.
    #[serde(default)]
    pub description: Option<String>,
    /// Rationale for the requirement.
    #[serde(default)]
    pub rationale: Option<String>,
    /// Verification methods.
    #[serde(default)]
    pub verification_methods: Vec<String>,
}

// ============================================================================
// Low-Level Requirements (LLR)
// ============================================================================

/// LLR TOML file structure.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct LlrFile {
    /// Schema version label for the file.
    pub schema: Schema,
    /// Document-level metadata (id + revision).
    pub meta: TraceMeta,
    /// Requirement entries in this file.
    pub requirements: Vec<LlrEntry>,
}

/// A Low-Level Requirement entry.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct LlrEntry {
    /// Machine-stable UUID.
    #[serde(default)]
    pub uid: Option<String>,
    /// Namespace prefix.
    #[serde(default)]
    pub ns: Option<String>,
    /// Human-readable ID.
    pub id: String,
    /// Requirement title.
    pub title: String,
    /// Owner.
    #[serde(default)]
    pub owner: Option<String>,
    /// Sort key for deterministic ordering.
    #[serde(default)]
    pub sort_key: Option<i64>,
    /// UIDs of HLRs this LLR traces to.
    pub traces_to: Vec<String>,
    /// Source reference.
    #[serde(default)]
    pub source: Option<String>,
    /// Implementation modules (DO-178C Table A-4 traceability).
    #[serde(default)]
    pub modules: Vec<String>,
    /// Whether this is a derived requirement.
    #[serde(default)]
    pub derived: bool,
    /// Requirement description.
    #[serde(default)]
    pub description: Option<String>,
    /// Rationale for derived requirements.
    #[serde(default)]
    pub rationale: Option<String>,
    /// Verification methods.
    #[serde(default)]
    pub verification_methods: Vec<String>,
}

// ============================================================================
// Test Cases
// ============================================================================

/// Tests TOML file structure.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct TestsFile {
    /// Schema version label for the file.
    pub schema: Schema,
    /// Document-level metadata (id + revision).
    pub meta: TraceMeta,
    /// Test-case entries in this file.
    pub tests: Vec<TestEntry>,
}

/// A Test case entry.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct TestEntry {
    /// Machine-stable UUID.
    #[serde(default)]
    pub uid: Option<String>,
    /// Namespace prefix.
    #[serde(default)]
    pub ns: Option<String>,
    /// Human-readable ID.
    pub id: String,
    /// Test title.
    pub title: String,
    /// Owner.
    #[serde(default)]
    pub owner: Option<String>,
    /// Sort key for deterministic ordering.
    #[serde(default)]
    pub sort_key: Option<i64>,
    /// UIDs of LLRs this test traces to.
    pub traces_to: Vec<String>,
    /// Test description.
    #[serde(default)]
    pub description: Option<String>,
    /// Test category (used to group objectives in compliance reports).
    #[serde(default)]
    pub category: Option<String>,
    /// Test selector (test function path for CI execution).
    #[serde(default)]
    pub test_selector: Option<String>,
    /// Source reference.
    #[serde(default)]
    pub source: Option<String>,
}

// ============================================================================
// Derived Requirements
// ============================================================================

/// Derived requirements TOML file structure.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct DerivedFile {
    /// Schema version label for the file.
    pub schema: Schema,
    /// Document-level metadata (id + revision).
    pub meta: TraceMeta,
    /// Derived-requirement entries in this file.
    pub requirements: Vec<DerivedEntry>,
}

/// A Derived Requirement entry.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct DerivedEntry {
    /// Machine-stable UUID.
    #[serde(default)]
    pub uid: Option<String>,
    /// Human-readable ID.
    pub id: String,
    /// Requirement title.
    pub title: String,
    /// Owner.
    #[serde(default)]
    pub owner: Option<String>,
    /// Source of derivation (e.g., `implementation`, `design`).
    #[serde(default)]
    pub source: Option<String>,
    /// Requirement description.
    #[serde(default)]
    pub description: Option<String>,
    /// Rationale for the derived requirement.
    #[serde(default)]
    pub rationale: Option<String>,
    /// Safety impact level (`none`, `low`, `medium`, `high`).
    #[serde(default)]
    pub safety_impact: Option<String>,
    /// Sort key for deterministic ordering.
    #[serde(default)]
    pub sort_key: Option<i64>,
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
    fn test_llr_entry_fields() -> anyhow::Result<()> {
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
        Ok(())
    }
}
