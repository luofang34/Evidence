//! Structural coverage types + cargo-llvm-cov JSON parser.
//!
//! The bundle's `coverage/coverage_summary.json` artifact is a
//! [`CoverageReport`] — a typed `measurements: Vec<Measurement>`
//! shape, NOT a C-style flat `{branches_covered, branches_total}`.
//! Rationale: Rust pattern-match decisions (arXiv 2409.08708
//! Section 4) can't be encoded as flat condition/decision counts,
//! so the wire format reserves shape for per-file decision +
//! condition vectors from the start. Today only `statement` and
//! `branch` levels emit; `pattern_decision` and `mcdc` are valid
//! enum values with empty emit sites until rustc's MC/DC support
//! stabilizes.
//!
//! LLR-053.

mod llvm_cov_json;
mod report;

pub use llvm_cov_json::{LlvmCovParseError, parse_llvm_cov_export};
pub use report::{
    BranchCoverage, ConditionCoverage, CoverageLevel, CoverageReport, DecisionCoverage,
    FileMeasurement, LineCoverage, Measurement,
};
