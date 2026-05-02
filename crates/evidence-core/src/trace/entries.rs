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
///
/// `HlrEntry` also doubles as the System-Requirement entry shape when
/// loaded from `sys.toml` — the layer is signaled by the source
/// filename, not by a struct field. Both layers share every field
/// below; the only cross-layer difference is that SYS entries have
/// empty `traces_to` (nothing above system level), while HLRs
/// optionally trace upward to SYS UIDs.
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
    /// UIDs of System Requirements this HLR traces up to. Empty for
    /// tool-internal requirements with no System-level parent, and
    /// always empty when this entry itself is a System Requirement
    /// (loaded from `sys.toml`).
    #[serde(default)]
    pub traces_to: Vec<String>,
    /// CLI verbs / named observable contracts this HLR governs. Must
    /// be subset of [`KNOWN_SURFACES`](crate::trace::KNOWN_SURFACES);
    /// the complementary constraint (every `KNOWN_SURFACES` entry is
    /// claimed by ≥1 HLR) is enforced in the Link-phase validator.
    /// LLR-038.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub surfaces: Vec<String>,
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
    /// Requirement description.
    #[serde(default)]
    pub description: Option<String>,
    /// Verification methods.
    #[serde(default)]
    pub verification_methods: Vec<String>,
    /// Diagnostic codes this LLR is responsible for — the "this LLR
    /// owns those codes" declaration that closes the
    /// code↔requirement loop (LLR-031). Every entry must
    /// appear in [`evidence_core::RULES`](crate::RULES); every entry in
    /// `RULES` (minus
    /// [`RESERVED_UNCLAIMED_CODES`](crate::RESERVED_UNCLAIMED_CODES))
    /// must appear in at least one LLR's `emits`. Bijection enforced
    /// by `diagnostic_codes_locked::every_code_is_claimed_by_an_llr`.
    ///
    /// LLRs describing pure structure (schema shapes, config
    /// loaders) that don't emit diagnostic codes may leave this
    /// empty. Default empty + `skip_serializing_if = "Vec::is_empty"`
    /// so existing external LLR files and fixtures deserialize and
    /// serialize unchanged.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub emits: Vec<String>,
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
    ///
    /// **Legacy shape**: kept for back-compat with the 1:1 TEST-to-fn
    /// convention. New code paths should prefer `test_selectors` for
    /// the N:M case. LLR-039.
    ///
    /// **Deprecation timeline**: stays through the pre-1.0 window —
    /// no schema constants churn until 1.0 ships (see
    /// `schema_versions.rs`). The 1.0 shape removes this field; a
    /// pre-1.0 migration PR does the single-fn → `test_selectors =
    /// ["X"]` rewrite across all in-tree TOML, coordinated with the
    /// `TRACE` version bump.
    #[serde(default)]
    pub test_selector: Option<String>,
    /// Additional selectors — enables one TEST entry to verify
    /// multiple `#[test] fn`s, or one fn to verify multiple TESTs.
    /// LLR-039. Used as the additive extension to
    /// `test_selector`; the resolver walks both as a union.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub test_selectors: Vec<String>,
    /// Source reference.
    #[serde(default)]
    pub source: Option<String>,
}

impl TestEntry {
    /// All selectors for this test entry — the legacy singular
    /// `test_selector` merged with the plural `test_selectors`.
    /// Deduped via `BTreeSet` and returned sorted for deterministic
    /// iteration. LLR-039.
    pub fn all_selectors(&self) -> Vec<String> {
        let mut set: std::collections::BTreeSet<String> =
            self.test_selectors.iter().cloned().collect();
        if let Some(s) = &self.test_selector {
            set.insert(s.clone());
        }
        set.into_iter().collect()
    }
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
            traces_to: vec!["sys-parent-uuid".to_string()],
            surfaces: vec![],
        };
        assert_eq!(hlr.id, "HLR-001");
        assert!(hlr.uid.is_some());
        assert_eq!(hlr.description.as_deref(), Some("A test requirement"));
        assert_eq!(hlr.rationale.as_deref(), Some("Because we need it"));
        assert_eq!(hlr.traces_to, vec!["sys-parent-uuid".to_string()]);
    }

    /// `HlrEntry.traces_to` is `#[serde(default)]`, so a TOML file
    /// without a `traces_to` key still parses — required for
    /// backwards compatibility with every -#44 hlr.toml.
    #[test]
    fn test_hlr_entry_traces_to_defaults_to_empty() {
        let toml_without_traces_to = r#"
            id = "HLR-LEGACY"
            title = "Legacy requirement with no upward trace"
        "#;
        let entry: HlrEntry =
            toml::from_str(toml_without_traces_to).expect("parse without traces_to");
        assert!(entry.traces_to.is_empty(), "default must be empty Vec");
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
            description: Some("An LLR description".to_string()),
            verification_methods: vec!["test".to_string()],
            emits: vec!["VERIFY_HASH_MISMATCH".to_string()],
        };
        assert_eq!(llr.id, "LLR-001");
        assert_eq!(llr.description.as_deref(), Some("An LLR description"));
        assert_eq!(llr.emits, vec!["VERIFY_HASH_MISMATCH".to_string()]);
    }
}
