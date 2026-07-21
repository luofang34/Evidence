//! Unit tests for the corpus index, graph invariants, layout
//! agnosticism, and legacy-trace parity (TEST-119..122).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

mod graph_layout;
mod graph_validation;
mod index;
mod legacy_parity;
mod records;
