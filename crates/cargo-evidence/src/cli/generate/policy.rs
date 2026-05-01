//! Boundary-policy gates for `cargo evidence generate`.
//!
//! Two stages, wrapped by [`enforce_boundary_policy`]:
//!
//! 1. [`assert_policy_implementable`] — refuses a run when
//!    `boundary.toml` enables a rule whose real enforcement hasn't
//!    shipped yet, so bundles cannot silently overclaim what was
//!    checked.
//! 2. The actual cargo-metadata-backed check for each implemented
//!    rule. Today: `no_out_of_scope_deps`, `forbid_build_rs`,
//!    `forbid_proc_macros`.
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
    // `cargo metadata` reads from cwd + upward-walk; the CLI is
    // invoked at the workspace root so cwd is correct. Passing "."
    // is documentary for the library API.
    let workspace_root = Path::new(".");

    if derived.policy.no_out_of_scope_deps {
        match evidence_core::check_no_out_of_scope_deps(&derived.in_scope_crates, workspace_root) {
            Ok(()) => {}
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
                return fail(json_output, profile, msg).map(Some);
            }
            Err(e) => {
                return Err(anyhow::Error::new(e).context("running no_out_of_scope_deps check"));
            }
        }
    }

    if derived.policy.forbid_build_rs {
        match evidence_core::check_no_build_rs(&derived.in_scope_crates, workspace_root) {
            Ok(()) => {}
            Err(evidence_core::BoundaryCheckError::ForbiddenBuildRs { violations, .. }) => {
                let lines: Vec<String> = violations
                    .iter()
                    .map(|v| match &v.links {
                        Some(l) => {
                            format!("  - {} has build.rs (links = \"{}\")", v.crate_name, l)
                        }
                        None => format!("  - {} has build.rs", v.crate_name),
                    })
                    .collect();
                let msg = format!(
                    "boundary policy violation: `forbid_build_rs` is enabled and \
                     {} in-scope crate(s) carry a build.rs (host-side build code \
                     breaks deterministic compilation):\n{}\n\
                     Either remove the build script, move the crate out of \
                     `scope.in_scope`, or set `forbid_build_rs = false` if the \
                     project's DAL allows it.",
                    violations.len(),
                    lines.join("\n")
                );
                return fail(json_output, profile, msg).map(Some);
            }
            Err(e) => return Err(anyhow::Error::new(e).context("running forbid_build_rs check")),
        }
    }

    if derived.policy.forbid_proc_macros {
        match evidence_core::check_no_proc_macros(&derived.in_scope_crates, workspace_root) {
            Ok(()) => {}
            Err(evidence_core::BoundaryCheckError::ForbiddenProcMacro { violations, .. }) => {
                let lines: Vec<String> = violations
                    .iter()
                    .map(|v| format!("  - {}", v.crate_name))
                    .collect();
                let msg = format!(
                    "boundary policy violation: `forbid_proc_macros` is enabled and \
                     {} in-scope crate(s) expose proc-macro targets (compile-time \
                     code synthesis is not auditable from the version-controlled \
                     tree):\n{}\n\
                     Either drop the proc-macro target, move the crate out of \
                     `scope.in_scope`, or set `forbid_proc_macros = false` if the \
                     project's DAL allows it.",
                    violations.len(),
                    lines.join("\n")
                );
                return fail(json_output, profile, msg).map(Some);
            }
            Err(e) => {
                return Err(anyhow::Error::new(e).context("running forbid_proc_macros check"));
            }
        }
    }

    Ok(None)
}

/// DAL-A qualification gate: refuse to assemble a cert/record bundle
/// when any in-scope crate is at DAL-A and the policy does not record
/// an [`evidence_core::AuxiliaryMcdcTool`] reference. DO-178C Annex A
/// Table A-7 Obj-7 (MC/DC) is required at DAL-A; stable Rust cannot
/// emit MC/DC instrumentation today (rust-lang/rust#144999 removed
/// the unstable flag), so the only viable path is to record an
/// external qualified tool's evidence by reference.
///
/// On `Profile::Dev`, fires a Warning-severity diagnostic and
/// returns `Ok(None)` — dev iteration is unblocked. On
/// `Profile::Cert` / `Profile::Record`, fails the run with a
/// JSON/text envelope so the caller can short-circuit.
pub(super) fn enforce_dal_qualification(
    derived: &BoundaryDerived,
    profile: Profile,
    json_output: bool,
) -> Result<Option<i32>> {
    match evidence_core::check_dal_a_mcdc_evidence(
        &derived.dal_map,
        derived.auxiliary_mcdc_tool.as_ref(),
    ) {
        Ok(()) => Ok(None),
        Err(evidence_core::BoundaryCheckError::DalAMissingAuxiliaryMcdc {
            dal_a_crates, ..
        }) => {
            let lines: Vec<String> = dal_a_crates.iter().map(|c| format!("  - {}", c)).collect();
            let msg = format!(
                "DAL-A qualification gap: stable Rust cannot emit MC/DC \
                 instrumentation (rust-lang/rust#144999, merged 2025-08-08). \
                 {} in-scope crate(s) at DAL-A but no `[dal.auxiliary_mcdc_tool]` \
                 entry in cert/boundary.toml records an external qualified MC/DC \
                 tool's evidence:\n{}\n\
                 Either: (a) record the auxiliary tool reference in \
                 boundary.toml's `[dal]` section (name, qualification_id, \
                 report) so the bundle binds external MC/DC evidence by \
                 reference, OR (b) lower the affected crate(s) to DAL-B \
                 (which does not require MC/DC), OR (c) wait for upstream \
                 rustc to reintroduce MC/DC instrumentation \
                 (tracking: rust-lang/rust#124144).",
                dal_a_crates.len(),
                lines.join("\n")
            );
            // dev profile downgrades to a stderr warning + continue;
            // cert/record fails the run.
            if matches!(profile, Profile::Dev) {
                eprintln!("warning: {}", msg);
                Ok(None)
            } else {
                fail(json_output, profile, msg).map(Some)
            }
        }
        Err(e) => Err(anyhow::Error::new(e).context("running DAL-A MC/DC qualification gate")),
    }
}
