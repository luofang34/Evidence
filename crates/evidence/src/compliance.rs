//! Per-crate DO-178C compliance reporting.
//!
//! Split across private sibling files under `compliance/`:
//!
//! | Sub-module         | Concern                                              |
//! |--------------------|------------------------------------------------------|
//! | `applicability`    | `Applicability` enum (NotApplicable / Required / RI) |
//! | `objective`        | `Objective` struct + `applicability_for`             |
//! | `objectives_table` | `OBJECTIVES` — DO-178C Annex A Tables A-3..A-7       |
//! | `report`           | `ObjectiveStatus`, `ComplianceSummary`, `ComplianceReport`, `CrateEvidence` |
//! | `status`           | `determine_objective_status` + Table A-7 helper      |
//! | `generator`        | `generate_compliance_report` — top-level entry point |
//!
//! Re-exports below keep the crate's public API flat — consumers
//! continue to `use evidence::compliance::{ComplianceReport, …}`
//! without caring about the split.

mod applicability;
mod generator;
mod objective;
mod objectives_table;
mod report;
mod status;

pub use applicability::Applicability;
pub use generator::generate_compliance_report;
pub use objective::Objective;
pub use objectives_table::OBJECTIVES;
pub use report::{ComplianceReport, ComplianceSummary, CrateEvidence, ObjectiveStatus};
