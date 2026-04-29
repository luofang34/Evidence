//! Unit tests for `coverage_phase`'s CLI-specific helpers
//! (`resolve_choice`, `levels_for_choice`). The aggregator and
//! threshold-dispatcher tests live in
//! `evidence_core::coverage::thresholds::tests` — moved there in
//! the discoverability refactor so a reviewer can reach them via
//! `cargo test --workspace <pattern>` without `--all-targets` or
//! `--bins`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use evidence_core::CoverageLevel;

use super::*;

#[test]
fn resolve_choice_explicit_wins_over_profile_default() {
    assert_eq!(
        resolve_choice(Some(CoverageChoice::None), Profile::Cert),
        CoverageChoice::None
    );
    assert_eq!(
        resolve_choice(Some(CoverageChoice::Branch), Profile::Dev),
        CoverageChoice::Branch
    );
}

#[test]
fn resolve_choice_defaults_by_profile_when_unset() {
    assert_eq!(resolve_choice(None, Profile::Dev), CoverageChoice::None);
    assert_eq!(resolve_choice(None, Profile::Cert), CoverageChoice::Branch);
    assert_eq!(
        resolve_choice(None, Profile::Record),
        CoverageChoice::Branch
    );
}

#[test]
fn levels_for_choice_maps_as_expected() {
    assert!(levels_for_choice(CoverageChoice::None).is_empty());
    assert_eq!(
        levels_for_choice(CoverageChoice::Line),
        vec![CoverageLevel::Statement]
    );
    assert_eq!(
        levels_for_choice(CoverageChoice::Branch),
        vec![CoverageLevel::Branch]
    );
    assert_eq!(
        levels_for_choice(CoverageChoice::All),
        vec![CoverageLevel::Statement, CoverageLevel::Branch]
    );
}
