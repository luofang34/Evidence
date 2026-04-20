//! Boundary-policy gates for `cargo evidence generate`.
//!
//! Two stages, wrapped by [`enforce_boundary_policy`]:
//!
//! 1. [`assert_policy_implementable`] — refuses a run when
//!    `boundary.toml` enables a rule whose real enforcement hasn't
//!    shipped yet, so bundles cannot silently overclaim what was
//!    checked.
//! 2. The actual cargo-metadata-backed check for each implemented
//!    rule. Today: `no_out_of_scope_deps`.
//!
//! Each additional rule lands here alongside deleting its branch
//! from [`evidence_core::BoundaryPolicy::unimplemented_enabled_rules`].
//! The orchestrator needs only one call to
//! [`enforce_boundary_policy`].

use std::path::Path;

use anyhow::Result;

use evidence_core::{BoundaryPolicy, Profile};

use super::fail;
use super::phases::BoundaryDerived;

/// Refuse a run when `boundary.toml` enables a rule this release
/// doesn't implement. Returns `Ok(Some(EXIT_ERROR))` with the
/// standard failure envelope on refusal, `Ok(None)` on success.
///
/// When real enforcement ships for a rule, delete its branch in
/// [`evidence_core::BoundaryPolicy::unimplemented_enabled_rules`] and
/// this gate stops rejecting it without further changes here.
pub(super) fn assert_policy_implementable(
    policy: &BoundaryPolicy,
    profile: Profile,
    json_output: bool,
) -> Result<Option<i32>> {
    let unimpl = policy.unimplemented_enabled_rules();
    if unimpl.is_empty() {
        return Ok(None);
    }
    let list = unimpl.join(", ");
    let msg = format!(
        "boundary.toml enables policy rules that this release does not enforce: [{list}]. \
         Set them to `false` (or remove the keys) until their enforcement lands, \
         so bundles do not silently overclaim what was checked."
    );
    fail(json_output, profile, msg).map(Some)
}

/// Run every currently-implemented boundary policy check.
///
/// First calls [`assert_policy_implementable`] so the orchestrator
/// has a single call site for every boundary check; then enforces
/// the rules the library actually implements today.
///
/// On violation, emits the standard failure envelope via [`fail`]
/// listing each (in-scope crate, offending dep) pair so the user
/// knows which edge to cut. `Ok(None)` on success, `Ok(Some(code))`
/// after a violation fail-envelope, `Err` only for tooling failures
/// (e.g. `cargo metadata` refused to run) so those surface
/// distinctly from rule violations.
pub(super) fn enforce_boundary_policy(
    derived: &BoundaryDerived,
    profile: Profile,
    json_output: bool,
) -> Result<Option<i32>> {
    if let Some(code) = assert_policy_implementable(&derived.policy, profile, json_output)? {
        return Ok(Some(code));
    }
    if !derived.policy.no_out_of_scope_deps {
        return Ok(None);
    }
    // `cargo metadata` reads from cwd + upward-walk; the CLI is
    // invoked at the workspace root so cwd is correct. Passing "."
    // is documentary for the library API.
    let workspace_root = Path::new(".");
    match evidence_core::check_no_out_of_scope_deps(&derived.in_scope_crates, workspace_root) {
        Ok(()) => Ok(None),
        Err(evidence_core::BoundaryCheckError::OutOfScopeDeps { violations, .. }) => {
            let lines: Vec<String> = violations
                .iter()
                .map(|v| {
                    format!(
                        "  - {} depends on out-of-scope crate {}",
                        v.crate_name, v.offending_dep
                    )
                })
                .collect();
            let msg = format!(
                "boundary policy violation: `no_out_of_scope_deps` is enabled and \
                 {} in-scope crate(s) reach workspace crates not in the in-scope list:\n{}\n\
                 Either add the listed crates to `scope.in_scope` in boundary.toml, \
                 or break the dependency.",
                violations.len(),
                lines.join("\n")
            );
            fail(json_output, profile, msg).map(Some)
        }
        Err(e) => Err(anyhow::Error::new(e).context("running no_out_of_scope_deps check")),
    }
}
