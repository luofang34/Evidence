//! Unit tests for `evidence_core::policy::boundary`. Lives in a
//! sibling file pulled in via `#[path]` so the parent stays under
//! the workspace 500-line limit.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use super::*;
use crate::policy::Dal;

#[test]
fn test_boundary_config_without_dal_section() {
    let toml_str = format!(
        r#"
[schema]
version = "{ver}"

[scope]
in_scope = ["my-crate"]

[policy]
no_out_of_scope_deps = true
"#,
        ver = crate::schema_versions::BOUNDARY
    );
    let config: BoundaryConfig = toml::from_str(&toml_str).unwrap();
    assert!(config.dal.is_none());
    // No [dal] section ⇒ the crate is unclassified, not DAL-D.
    assert_eq!(config.dal_map()["my-crate"], AssuranceLevel::Unclassified);
    assert_eq!(config.assurance_selection(), None);
}

#[test]
fn test_boundary_config_with_dal_section() {
    let toml_str = format!(
        r#"
[schema]
version = "{ver}"

[scope]
in_scope = ["flight-core", "telemetry"]

[policy]
no_out_of_scope_deps = true

[dal]
default_dal = "C"

[dal.crate_overrides]
"flight-core" = "A"
"#,
        ver = crate::schema_versions::BOUNDARY
    );
    let config: BoundaryConfig = toml::from_str(&toml_str).unwrap();
    let dal = config.dal.as_ref().expect("[dal] section present");
    assert_eq!(dal.default_dal, Some(Dal::C));
    assert_eq!(dal.crate_overrides["flight-core"], Dal::A);
    assert_eq!(config.dal_map()["flight-core"], AssuranceLevel::DalA);
    assert_eq!(config.dal_map()["telemetry"], AssuranceLevel::DalC);
    // Highest rigor in scope wins the selection.
    let selection = config.assurance_selection().expect("explicit selection");
    assert_eq!(selection.level, AssuranceLevel::DalA);
    assert_eq!(selection.standard, StandardEdition::Do178c);
}

/// Fail-closed preconditions (LLR-109): the selection exists only
/// when `[dal]` is present with an explicit `default_dal` AND
/// `scope.in_scope` is non-empty.
#[test]
fn assurance_selection_requires_explicit_dal_and_scope() {
    let mk = |dal_section: &str, in_scope: &str| {
        toml::from_str::<BoundaryConfig>(&format!(
            r#"
[schema]
version = "{ver}"

[scope]
in_scope = [{in_scope}]

[policy]
no_out_of_scope_deps = false
{dal_section}
"#,
            ver = crate::schema_versions::BOUNDARY
        ))
        .unwrap()
    };

    // No [dal] at all → None.
    assert_eq!(mk("", "\"a\"").assurance_selection(), None);
    // [dal] without default_dal → None.
    assert_eq!(mk("[dal]", "\"a\"").assurance_selection(), None);
    // default_dal but empty in_scope → None.
    assert_eq!(
        mk("[dal]\ndefault_dal = \"A\"", "").assurance_selection(),
        None
    );
    // Fully explicit → Some.
    let sel = mk("[dal]\ndefault_dal = \"B\"", "\"a\"")
        .assurance_selection()
        .expect("explicit default_dal + in_scope");
    assert_eq!(sel.level, AssuranceLevel::DalB);
    // default_empty (the load_or_default fallback shape) → None.
    assert_eq!(BoundaryConfig::default_empty().assurance_selection(), None);
}

fn policy_all(
    no_out_of_scope_deps: bool,
    forbid_build_rs: bool,
    forbid_proc_macros: bool,
) -> BoundaryPolicy {
    BoundaryPolicy {
        no_out_of_scope_deps,
        forbid_build_rs,
        forbid_proc_macros,
    }
}

#[test]
fn unimplemented_enabled_rules_empty_when_all_disabled() {
    let p = policy_all(false, false, false);
    assert!(p.unimplemented_enabled_rules().is_empty());
}

#[test]
fn unimplemented_enabled_rules_is_empty_for_every_combination() {
    // Every `BoundaryPolicy` flag now has real enforcement in
    // `evidence_core::boundary_check`. The preflight refusal
    // surface is empty — the library-level checks fire on
    // violations directly. If a future flag lands without
    // enforcement, this test fires immediately.
    for (a, b, c) in [
        (false, false, false),
        (true, false, false),
        (false, true, false),
        (false, false, true),
        (true, true, true),
    ] {
        assert!(
            policy_all(a, b, c).unimplemented_enabled_rules().is_empty(),
            "expected empty for ({a}, {b}, {c})"
        );
    }
}

/// default_empty() must never trip the preflight — that would
/// mean `cargo evidence generate` fails cold on a workspace
/// without a `cert/boundary.toml`, which is exactly the path
/// `load_or_default` exists to support.
#[test]
fn unimplemented_enabled_rules_empty_for_default_empty_config() {
    let cfg = BoundaryConfig::default_empty();
    assert!(cfg.policy.unimplemented_enabled_rules().is_empty());
}
