//! Traceability types and functions for DO-178C-style requirements
//! linkage: HLR → LLR → Test, plus optional Derived requirements.
//!
//! The module is split across private sibling files under `trace/`:
//!
//! | Sub-module   | Concern                                                   |
//! |--------------|-----------------------------------------------------------|
//! | `entries`        | TOML data types (`HlrEntry`, `LlrEntry`, `TestEntry`, …)  |
//! | `read`           | Reading TOML files into those types                       |
//! | `uuid`           | Assigning + back-filling UUIDs on entries                 |
//! | `validation`     | Cross-tier link validation with policy gates              |
//! | `selector_check` | Optional resolution of `test_selector` vs workspace source |
//! | `matrix`         | Deterministic Markdown traceability matrix generation     |
//!
//! Re-exports below keep the crate's public API flat — every
//! consumer can continue to `use evidence_core::trace::HlrEntry` without
//! caring about the split.

mod entries;
mod matrix;
mod read;
mod requirement_report;
mod selector_check;
mod surfaces;
mod test_backlinks;
mod uuid;
mod validation;

pub use entries::{
    DerivedEntry, DerivedFile, HlrEntry, HlrFile, LlrEntry, LlrFile, Schema, TestEntry, TestsFile,
    TraceMeta,
};
pub use matrix::generate_traceability_matrix;
pub use read::{TraceFiles, read_all_trace_files, read_toml};
pub use requirement_report::{RequirementStatus, build_requirement_report};
pub use selector_check::{UnresolvedSelector, resolve_test_selectors};
pub use surfaces::KNOWN_SURFACES;
pub use test_backlinks::resolve_llr_backlinks;
pub use uuid::{
    assign_missing_uuids_derived, assign_missing_uuids_hlr, assign_missing_uuids_llr,
    assign_missing_uuids_test, backfill_uuids,
};
pub use validation::{
    LinkError, TraceValidationError, validate_trace_links, validate_trace_links_with_policy,
};
