//! Core traits for the evidence system.
//!
//! This module defines the core traits that components
//! of the evidence system must implement.

/// Trait for types that can be hashed for evidence.
pub trait Hashable {
    /// Compute the hash of this item.
    fn compute_hash(&self) -> anyhow::Result<String>;
}

/// Trait for types that can be serialized to evidence format.
pub trait ToEvidence {
    /// Convert to evidence JSON.
    fn to_evidence(&self) -> anyhow::Result<serde_json::Value>;
}
