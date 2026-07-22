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
pub mod context;
pub mod corpus;
pub mod coverage;
pub mod diagnostic;
pub mod diff;
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

mod public_api;
pub use public_api::*;
