//! Evidence — build-evidence and reproducibility verification library.
//!
//! Captures build environments, produces deterministic evidence
//! bundles, and verifies bundles for safety-critical certification
//! workflows.
//!
//! # Public surface
//!
//! The crate root re-exports the items most external consumers reach
//! for. Items hidden from rustdoc (`#[doc(hidden)]`) are
//! implementation details — they remain reachable via their owning
//! module (`evidence_core::coverage::FileMeasurement`, etc.) for
//! workspace internals, but are **not** part of the stable public
//! API and may change between any 0.x.y release.
//!
//! # Modules
//!
//! - [`bundle`] - Evidence bundle creation and management
//! - [`compliance`] - Per-crate DO-178C compliance reporting
//! - [`coverage`] - Structural coverage data types
//! - [`diagnostic`] - Agent-consumable diagnostic format + trait
//! - [`mod@env`] - Build environment fingerprinting
//! - [`git`] - Git repository state capture
//! - [`hash`] - Cryptographic hashing utilities
//! - [`policy`] - Configuration and policy types
//! - [`trace`] - Requirements traceability (HLR/LLR/Test)
//! - [`traits`] - Core abstraction traits
//! - [`verify`] - Bundle verification
//!
//! # Example
//!
//! ```rust,ignore
//! use evidence_core::{git::GitSnapshot, env::EnvFingerprint, verify::verify_bundle};
//! use std::path::Path;
//!
//! // Capture current state (strict=true for cert/record profiles)
//! let git = GitSnapshot::capture(true)?;
//! let env = EnvFingerprint::capture("cert", true)?;
//!
//! // Verify an existing bundle
//! let result = verify_bundle(Path::new("evidence/bundle-20240101"))?;
//! ```

pub mod boundary_check;
pub mod bundle;
pub mod cargo_metadata;
pub mod compliance;
pub mod coverage;
pub mod diagnostic;
pub mod env;
pub mod floors;
pub mod git;
pub mod hash;
pub mod policy;
pub mod rules;
pub mod schema;
pub mod schema_versions;
pub mod trace;
pub mod traits;
pub mod util;
pub mod verify;

// ===== Public surface =====
//
// Items deliberately curated for external library consumers. The
// per-module re-exports below cover the common scenarios: verify a
// bundle, build a bundle, generate compliance, read trace, query
// rules, evaluate coverage thresholds.

pub use boundary_check::{
    BoundaryCheckError, check_dal_a_mcdc_evidence, check_no_build_rs, check_no_out_of_scope_deps,
    check_no_proc_macros,
};
pub use bundle::{
    EvidenceBuildConfig, EvidenceBuilder, EvidenceIndex, TestSummary, ToolCommandFailure,
    parse_cargo_test_output_detailed, sign_bundle, verify_bundle_signature,
};
pub use compliance::{
    Applicability, ComplianceReport, ComplianceSummary, CrateEvidence, OBJECTIVES, ObjectiveStatus,
    ObjectiveStatusKind, generate_compliance_report,
};
pub use coverage::{
    CoverageLevel, CoverageReport, CoverageThresholdViolation, evaluate_thresholds,
    parse_llvm_cov_export,
};
pub use diagnostic::{Diagnostic, DiagnosticCode, Location, Severity, TERMINAL_CODES};
pub use env::{EnvFingerprint, Host};
pub use floors::{FloorsConfig, current_measurements};
pub use git::{GitSnapshot, RealGitProvider};
pub use policy::{
    AuxiliaryMcdcTool, BoundaryConfig, BoundaryPolicy, Dal, DalConfig, EvidencePolicy, Profile,
    TracePolicy, load_trace_roots,
};
pub use rules::{Domain, RULES, RuleEntry};
pub use trace::{
    DerivedEntry, DerivedFile, HlrEntry, HlrFile, LlrEntry, LlrFile, TestEntry, TestsFile,
    generate_traceability_matrix, read_all_trace_files, validate_trace_links,
    validate_trace_links_with_policy,
};
pub use traits::GitProvider;
pub use verify::{VerifyError, VerifyResult, verify_bundle, verify_bundle_with_key};

// ===== Implementation detail =====
//
// Re-exported at the crate root for workspace-internal callers
// (cargo-evidence, evidence-mcp, contract tests). External library
// consumers should treat these as unstable — `cargo-evidence` itself
// is the stable contract. Reach into `evidence_core::<module>::*` if
// you genuinely need one and pin the workspace version.

#[doc(hidden)]
pub use boundary_check::{BoundaryViolation, BuildRsViolation, ProcMacroViolation};
#[doc(hidden)]
pub use bundle::{CommandRecord, parse_cargo_test_output};
#[doc(hidden)]
pub use cargo_metadata::{
    CargoMetadataProjection, PackageProjection, ProjectionError, TargetProjection,
    check_build_rs_in_projection, check_proc_macros_in_projection,
};
#[doc(hidden)]
pub use coverage::{
    BranchCoverage, ConditionCoverage, DecisionCoverage, FileMeasurement, LineCoverage,
    LlvmCovParseError, Measurement, aggregate_branches_percent, aggregate_lines_percent,
};
#[doc(hidden)]
pub use diagnostic::FixHint;
#[doc(hidden)]
pub use env::DeterministicManifest;
#[doc(hidden)]
pub use floors::LoadOutcome;
#[doc(hidden)]
pub use git::{check_shallow_clone, is_dirty_or_unknown};
#[doc(hidden)]
pub use hash::{sha256, sha256_file};
#[doc(hidden)]
pub use policy::DalCoverageThresholds;
#[doc(hidden)]
pub use rules::{
    HAND_EMITTED_CLI_CODES, HAND_EMITTED_MCP_CODES, RESERVED_UNCLAIMED_CODES, rules_json,
};
#[doc(hidden)]
pub use trace::{
    Schema, TraceFiles, TraceMeta, assign_valid_uuids_derived, assign_valid_uuids_hlr,
    assign_valid_uuids_llr, assign_valid_uuids_test, backfill_uuids, read_toml,
};
#[doc(hidden)]
pub use util::normalize_bundle_path;
