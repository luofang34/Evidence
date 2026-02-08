//! Structural coverage data types.
//!
//! This module defines types for representing structural coverage data
//! (statement, decision, MC/DC) from tools like cargo-llvm-cov.
//! Phase 4 provides the API surface only; actual collection is future work.

use serde::{Deserialize, Serialize};

/// Coverage level required by DAL.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum CoverageLevel {
    /// Statement coverage (DAL-C minimum)
    Statement,
    /// Decision/branch coverage (DAL-B minimum)
    Decision,
    /// Modified Condition/Decision Coverage (DAL-A minimum)
    Mcdc,
}

impl std::fmt::Display for CoverageLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CoverageLevel::Statement => write!(f, "statement"),
            CoverageLevel::Decision => write!(f, "decision"),
            CoverageLevel::Mcdc => write!(f, "MC/DC"),
        }
    }
}

/// Per-crate coverage summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageSummary {
    /// Crate name
    pub crate_name: String,
    /// What level of coverage this data represents
    pub level: CoverageLevel,
    /// Lines/branches/conditions covered
    pub covered: u64,
    /// Total lines/branches/conditions
    pub total: u64,
    /// Coverage percentage (0.0 - 100.0)
    pub percentage: f64,
    /// Path to the raw coverage file in the bundle
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_file: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_coverage_level_ordering() {
        assert!(CoverageLevel::Mcdc > CoverageLevel::Decision);
        assert!(CoverageLevel::Decision > CoverageLevel::Statement);
    }

    #[test]
    fn test_coverage_level_display() {
        assert_eq!(CoverageLevel::Statement.to_string(), "statement");
        assert_eq!(CoverageLevel::Decision.to_string(), "decision");
        assert_eq!(CoverageLevel::Mcdc.to_string(), "MC/DC");
    }

    #[test]
    fn test_coverage_summary_serialization() {
        let summary = CoverageSummary {
            crate_name: "my-crate".to_string(),
            level: CoverageLevel::Statement,
            covered: 150,
            total: 200,
            percentage: 75.0,
            source_file: Some("coverage/my-crate.lcov".to_string()),
        };
        let json = serde_json::to_string(&summary).unwrap();
        let parsed: CoverageSummary = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.crate_name, "my-crate");
        assert_eq!(parsed.covered, 150);
        assert_eq!(parsed.percentage, 75.0);
    }
}
