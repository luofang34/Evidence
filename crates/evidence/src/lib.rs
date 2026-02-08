//! Evidence - Build Evidence and Reproducibility Verification Library
//!
//! This library provides tools for capturing build environments,
//! generating reproducible build evidence, and verifying builds
//! for safety-critical certification workflows.
//!
//! # Modules
//!
//! - [`bundle`] - Evidence bundle creation and management
//! - [`env`] - Build environment fingerprinting
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

pub mod bundle;
pub mod env;
pub mod git;
pub mod hash;
pub mod policy;
pub mod trace;
pub mod traits;
pub mod util;
pub mod verify;

// Re-export key types for convenience
pub use bundle::{
    parse_cargo_test_output, sign_bundle, verify_bundle_signature, CommandRecord,
    EvidenceBuildConfig, EvidenceBuilder, EvidenceIndex, TestSummary,
};
pub use env::EnvFingerprint;
pub use git::{GitSnapshot, RealGitProvider};
pub use hash::{sha256, sha256_file};
pub use policy::{BoundaryConfig, Profile, ProfileConfig, TracePolicy};
pub use trace::{
    assign_missing_uuids_derived, assign_missing_uuids_hlr, assign_missing_uuids_llr,
    assign_missing_uuids_test, backfill_uuids, generate_traceability_matrix, read_all_trace_files,
    read_toml, validate_trace_links, validate_trace_links_with_policy, DerivedEntry, DerivedFile,
    HlrEntry, HlrFile, LlrEntry, LlrFile, Schema, TestEntry, TestsFile, TraceFiles, TraceMeta,
};
pub use traits::{EnvironmentDetector, GitProvider};
pub use verify::{verify_bundle, verify_bundle_with_key, VerifyError, VerifyResult};
