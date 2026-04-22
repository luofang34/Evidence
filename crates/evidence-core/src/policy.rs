//! Policy and configuration types.
//!
//! Split across private sibling files under `policy/`:
//!
//! | Sub-module  | Concern                                                |
//! |-------------|--------------------------------------------------------|
//! | `profile`   | `Profile` enum + `ParseProfileError` + FromStr/Display |
//! | `dal`       | `Dal` enum + `DalConfig` + `ParseDalError`             |
//! | `boundary`  | `BoundaryConfig` + `Schema` + scope/policy + loaders   |
//! | `evidence`  | `EvidencePolicy::for_dal` + `TracePolicy`              |
//!
//! Re-exports below keep the crate's public API flat — consumers
//! continue to `use evidence_core::policy::{Profile, Dal, BoundaryConfig,
//! …}` without caring about the split.

mod boundary;
mod dal;
mod evidence;
mod profile;

pub use boundary::{
    BoundaryConfig, BoundaryPolicy, BoundaryScope, LoadBoundaryError, Schema, load_trace_roots,
};
pub use dal::{Dal, DalConfig, DalCoverageThresholds, ParseDalError};
pub use evidence::{EvidencePolicy, TracePolicy};
pub use profile::{ParseProfileError, Profile};
