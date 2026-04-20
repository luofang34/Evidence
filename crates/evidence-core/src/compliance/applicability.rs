//! `Applicability` — three-level objective applicability per DO-178C.

use serde::{Deserialize, Serialize};

/// DO-178C objective applicability level.
/// Three-level per standard: not applicable, required, or required with independence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Applicability {
    /// Objective does not apply at this DAL level
    NotApplicable,
    /// Objective is required
    Required,
    /// Objective is required with independent verification
    RequiredWithIndependence,
}
