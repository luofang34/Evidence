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
//! | `runtime_error` | `VerifyRuntimeError` enum + `DiagnosticCode` impl   |
//! | `bundle`        | orchestrator: `verify_bundle[_with_key]`            |
//! | `reproduction`  | `compare_reproduction` — reproduced-output equality |
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
mod errors_display;
mod llr_selectors;
mod output_manifest;
mod paths;
mod reproduction;
mod resolution_policy;
mod runtime_error;
mod source_baseline;
mod test_identity;
mod trace_evidence;

pub use bundle::{verify_bundle, verify_bundle_with_key};
pub use errors::{VerifyError, VerifyResult};
pub use paths::REQUIRED_FILES;
pub use reproduction::{ReproductionError, ReproductionFinding, compare_reproduction};
pub use runtime_error::VerifyRuntimeError;

// Plane-diff helpers shared with the bundle-diff engine
// (`crate::diff`) so both comparisons walk the same loading and
// key-diff logic instead of duplicating it (LLR-147).
pub(crate) use reproduction::{
    Plane, compare_recipe_fields, diff_digest_planes, read_digest_map, read_recipe,
};
