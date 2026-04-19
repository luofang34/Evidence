//! Evidence - Build Evidence and Reproducibility Verification Library
//!
//! This library provides tools for capturing build environments,
//! generating reproducible build evidence, and verifying builds
//! for safety-critical certification workflows.
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
//! use evidence::{git::GitSnapshot, env::EnvFingerprint, verify::verify_bundle};
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
pub mod compliance;
pub mod coverage;
pub mod diagnostic;
pub mod env;
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

// Re-export key types for convenience
pub use boundary_check::{BoundaryCheckError, BoundaryViolation, check_no_out_of_scope_deps};
pub use bundle::{
    CommandRecord, EvidenceBuildConfig, EvidenceBuilder, EvidenceIndex, TestSummary,
    parse_cargo_test_output, sign_bundle, verify_bundle_signature,
};
pub use compliance::{
    Applicability, ComplianceReport, ComplianceSummary, CrateEvidence, OBJECTIVES, ObjectiveStatus,
    ObjectiveStatusKind, generate_compliance_report,
};
pub use coverage::{CoverageLevel, CoverageSummary};
pub use diagnostic::{Diagnostic, DiagnosticCode, FixHint, Location, Severity, TERMINAL_CODES};
pub use env::{DeterministicManifest, EnvFingerprint, Host};
pub use git::{GitSnapshot, RealGitProvider, check_shallow_clone, is_dirty_or_unknown};
pub use hash::{sha256, sha256_file};
pub use policy::{
    BoundaryConfig, BoundaryPolicy, Dal, DalConfig, EvidencePolicy, Profile, TracePolicy,
    load_trace_roots,
};
pub use rules::{
    Domain, HAND_EMITTED_CLI_CODES, RESERVED_UNCLAIMED_CODES, RULES, RuleEntry, rules_json,
};
pub use trace::{
    DerivedEntry, DerivedFile, HlrEntry, HlrFile, LlrEntry, LlrFile, Schema, TestEntry, TestsFile,
    TraceFiles, TraceMeta, assign_missing_uuids_derived, assign_missing_uuids_hlr,
    assign_missing_uuids_llr, assign_missing_uuids_test, backfill_uuids,
    generate_traceability_matrix, read_all_trace_files, read_toml, validate_trace_links,
    validate_trace_links_with_policy,
};
pub use traits::GitProvider;
pub use util::normalize_bundle_path;
pub use verify::{VerifyError, VerifyResult, verify_bundle, verify_bundle_with_key};
