//! Deterministic read-only re-ingestion drift comparison
//! (LLR-176..LLR-178).
//!
//! Re-ingestion of verified frozen inputs under an explicitly
//! identified recipe produces candidate parser, patch, and
//! effective-graph planes; [`compare_reingestion`] compares them
//! against the committed baseline as a drift lint — never as a
//! source of truth and never as a baseline rewrite (DD-7). The
//! comparison is pure and read-only, reports closed typed
//! categories in a deterministic sorted order, treats timestamps,
//! absolute paths, map order, file layout, and diagnostic-only
//! source positions as non-semantic, and returns explicit equality
//! when every plane compares equal.
//!
//! Module map:
//!
//! - `error` — the flat [`DriftError`] prerequisite taxonomy
//! - `findings` — the closed [`DriftCategory`] set, typed
//!   [`DriftFinding`] context, the [`DriftReport`], and the
//!   canonical report rendering
//! - `nodes` — structural-key reconciliation and per-field
//!   node-plane comparison
//! - `patches` — patch, review, and candidate-effective planes
//! - `compare` — the [`compare_reingestion`] entry point

mod compare;
mod error;
mod findings;
mod node_locator;
mod nodes;
mod patches;

pub use compare::{DriftBaseline, ReingestionCandidate, compare_reingestion};
pub use error::DriftError;
pub use findings::{
    DriftCategory, DriftDetail, DriftFinding, DriftOutcome, DriftReport, render_report_canonical,
};

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
#[path = "drift/tests.rs"]
mod tests;
#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
#[path = "drift/tests_planes.rs"]
mod tests_planes;
#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
#[path = "drift/tests_support.rs"]
pub(crate) mod tests_support;
