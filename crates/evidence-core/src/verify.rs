//! Evidence bundle verification.
//!
//! Split across private sibling files under `verify/`:
//!
//! | Sub-module      | Concern                                             |
//! |-----------------|-----------------------------------------------------|
//! | `errors`        | `VerifyError` + `VerifyResult` structured types     |
//! | `paths`         | `is_safe_bundle_path` + `REQUIRED_FILES` constants  |
//! | `engine_source` | `check_engine_source` cross-shape                   |
//! | `cross_file`    | env.json ↔ index.json field consistency             |
//! | `consistency`   | trace_outputs / test_summary / dal_map cross-checks |
//! | `bundle`        | orchestrator: `verify_bundle[_with_key]`            |
//!
//! Re-exports below keep the crate's public API flat — consumers
//! continue to `use evidence_core::verify::{verify_bundle, VerifyError, …}`
//! without caring about the split.

mod bundle;
mod completeness;
mod consistency;
mod cross_file;
mod engine_source;
mod errors;
mod paths;

pub use bundle::{VerifyRuntimeError, verify_bundle, verify_bundle_with_key};
pub use errors::{VerifyError, VerifyResult};
pub use paths::REQUIRED_FILES;
