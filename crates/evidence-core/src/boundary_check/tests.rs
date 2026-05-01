//! Unit tests for `evidence_core::boundary_check`. Lives in a sibling
//! file pulled in via `#[path]` so the parent stays under the
//! workspace 500-line limit.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use super::*;

// Synthetic metadata fixtures. Cargo's real JSON has dozens of
// fields we don't use — serde drops them via default behavior,
// so the test fixtures only need to carry the keys `Metadata`
// actually deserializes.

fn pkg(name: &str, id: &str) -> serde_json::Value {
    serde_json::json!({"name": name, "id": id, "targets": []})
}

fn pkg_with_targets(
    name: &str,
    id: &str,
    targets: Vec<serde_json::Value>,
    links: Option<&str>,
) -> serde_json::Value {
    let mut v = serde_json::json!({"name": name, "id": id, "targets": targets});
    if let Some(l) = links {
        v["links"] = serde_json::json!(l);
    }
    v
}

fn target(name: &str, kinds: &[&str]) -> serde_json::Value {
    serde_json::json!({"name": name, "kind": kinds})
}

fn node(id: &str, dep_ids: &[&str]) -> serde_json::Value {
    let deps: Vec<serde_json::Value> = dep_ids
        .iter()
        .map(|d| serde_json::json!({"pkg": d}))
        .collect();
    serde_json::json!({"id": id, "deps": deps})
}

fn fixture(
    packages: Vec<serde_json::Value>,
    ws_members: Vec<&str>,
    nodes: Vec<serde_json::Value>,
) -> Metadata {
    let j = serde_json::json!({
        "packages": packages,
        "workspace_members": ws_members,
        "resolve": {"nodes": nodes},
    });
    serde_json::from_value(j).unwrap()
}

#[test]
fn no_violations_when_in_scope_depends_only_on_external_crates() {
    let m = fixture(
        vec![
            pkg("evidence", "path+file:///e#0.1.0"),
            pkg("serde", "registry+https://crates.io#serde@1"),
        ],
        vec!["path+file:///e#0.1.0"],
        vec![
            node(
                "path+file:///e#0.1.0",
                &["registry+https://crates.io#serde@1"],
            ),
            node("registry+https://crates.io#serde@1", &[]),
        ],
    );
    let v = find_out_of_scope_deps(&["evidence".into()], &m).unwrap();
    assert!(v.is_empty());
}

#[test]
fn no_violations_when_every_workspace_dep_is_in_scope() {
    let m = fixture(
        vec![
            pkg("cargo-evidence", "path+file:///ce#0.1.0"),
            pkg("evidence", "path+file:///e#0.1.0"),
        ],
        vec!["path+file:///ce#0.1.0", "path+file:///e#0.1.0"],
        vec![
            node("path+file:///ce#0.1.0", &["path+file:///e#0.1.0"]),
            node("path+file:///e#0.1.0", &[]),
        ],
    );
    let v = find_out_of_scope_deps(&["cargo-evidence".into(), "evidence".into()], &m).unwrap();
    assert!(v.is_empty());
}

#[test]
fn flags_direct_workspace_dep_not_in_scope() {
    let m = fixture(
        vec![
            pkg("cargo-evidence", "path+file:///ce#0.1.0"),
            pkg("evidence", "path+file:///e#0.1.0"),
        ],
        vec!["path+file:///ce#0.1.0", "path+file:///e#0.1.0"],
        vec![
            node("path+file:///ce#0.1.0", &["path+file:///e#0.1.0"]),
            node("path+file:///e#0.1.0", &[]),
        ],
    );
    let v = find_out_of_scope_deps(&["cargo-evidence".into()], &m).unwrap();
    assert_eq!(v.len(), 1);
    assert_eq!(
        v[0],
        BoundaryViolation {
            rule: "no_out_of_scope_deps",
            crate_name: "cargo-evidence".into(),
            offending_dep: "evidence".into(),
        }
    );
}

#[test]
fn flags_transitive_workspace_dep_not_in_scope() {
    let m = fixture(
        vec![
            pkg("a", "path+file:///a#0.1.0"),
            pkg("b", "path+file:///b#0.1.0"),
            pkg("c", "path+file:///c#0.1.0"),
        ],
        vec![
            "path+file:///a#0.1.0",
            "path+file:///b#0.1.0",
            "path+file:///c#0.1.0",
        ],
        vec![
            node("path+file:///a#0.1.0", &["path+file:///b#0.1.0"]),
            node("path+file:///b#0.1.0", &["path+file:///c#0.1.0"]),
            node("path+file:///c#0.1.0", &[]),
        ],
    );
    let v = find_out_of_scope_deps(&["a".into()], &m).unwrap();
    let names: Vec<&str> = v.iter().map(|x| x.offending_dep.as_str()).collect();
    assert_eq!(names, vec!["b", "c"]);
}

#[test]
fn typos_in_in_scope_are_reported() {
    let m = fixture(
        vec![pkg("evidence", "path+file:///e#0.1.0")],
        vec!["path+file:///e#0.1.0"],
        vec![node("path+file:///e#0.1.0", &[])],
    );
    let err = find_out_of_scope_deps(&["typo-crate".into()], &m).unwrap_err();
    assert!(matches!(err, BoundaryCheckError::UnknownInScopeCrate(name) if name == "typo-crate"));
}

#[test]
fn diamond_dep_is_deduplicated() {
    let m = fixture(
        vec![
            pkg("a", "path+file:///a#0.1.0"),
            pkg("b", "path+file:///b#0.1.0"),
            pkg("c", "path+file:///c#0.1.0"),
            pkg("d", "path+file:///d#0.1.0"),
        ],
        vec![
            "path+file:///a#0.1.0",
            "path+file:///b#0.1.0",
            "path+file:///c#0.1.0",
            "path+file:///d#0.1.0",
        ],
        vec![
            node(
                "path+file:///a#0.1.0",
                &["path+file:///b#0.1.0", "path+file:///c#0.1.0"],
            ),
            node("path+file:///b#0.1.0", &["path+file:///d#0.1.0"]),
            node("path+file:///c#0.1.0", &["path+file:///d#0.1.0"]),
            node("path+file:///d#0.1.0", &[]),
        ],
    );
    let v = find_out_of_scope_deps(&["a".into()], &m).unwrap();
    let ds: Vec<&str> = v
        .iter()
        .filter(|x| x.offending_dep == "d")
        .map(|x| x.offending_dep.as_str())
        .collect();
    assert_eq!(ds.len(), 1, "diamond dep should not double-count");
}

// ============================================================================
// forbid_build_rs / forbid_proc_macros — Layer 1 (kind-based detection)
// ============================================================================

/// TEST-075 (a): clean workspace — no build.rs, no proc-macro — passes.
#[test]
fn clean_workspace_passes_forbid_build_rs() {
    let m = fixture(
        vec![
            pkg_with_targets(
                "lib_a",
                "path+file:///a#0.1.0",
                vec![target("lib_a", &["lib"])],
                None,
            ),
            pkg_with_targets(
                "lib_b",
                "path+file:///b#0.1.0",
                vec![target("lib_b", &["lib"])],
                None,
            ),
        ],
        vec!["path+file:///a#0.1.0", "path+file:///b#0.1.0"],
        vec![],
    );
    let v = find_build_rs_violations(&["lib_a".into(), "lib_b".into()], &m).unwrap();
    assert!(v.is_empty());
}

/// TEST-075 (b): clean workspace — no proc-macro target — passes.
#[test]
fn clean_workspace_passes_forbid_proc_macros() {
    let m = fixture(
        vec![pkg_with_targets(
            "lib_a",
            "path+file:///a#0.1.0",
            vec![target("lib_a", &["lib"])],
            None,
        )],
        vec!["path+file:///a#0.1.0"],
        vec![],
    );
    let v = find_proc_macro_violations(&["lib_a".into()], &m).unwrap();
    assert!(v.is_empty());
}

/// TEST-076 (a): in-scope crate with build.rs fires the violation.
#[test]
fn in_scope_build_rs_fires_violation() {
    let m = fixture(
        vec![pkg_with_targets(
            "lib_a",
            "path+file:///a#0.1.0",
            vec![
                target("lib_a", &["lib"]),
                target("build-script-build", &["custom-build"]),
            ],
            None,
        )],
        vec!["path+file:///a#0.1.0"],
        vec![],
    );
    let v = find_build_rs_violations(&["lib_a".into()], &m).unwrap();
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].crate_name, "lib_a");
    assert!(v[0].links.is_none());
}

/// TEST-076 (b): build.rs + `links =` surfaces the links value in the
/// violation so the diagnostic message can carry it (Layer 2).
#[test]
fn in_scope_build_rs_with_links_surfaces_links_in_message() {
    let m = fixture(
        vec![pkg_with_targets(
            "ffi_user",
            "path+file:///f#0.1.0",
            vec![
                target("ffi_user", &["lib"]),
                target("build-script-build", &["custom-build"]),
            ],
            Some("libz"),
        )],
        vec!["path+file:///f#0.1.0"],
        vec![],
    );
    let v = find_build_rs_violations(&["ffi_user".into()], &m).unwrap();
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].crate_name, "ffi_user");
    assert_eq!(v[0].links.as_deref(), Some("libz"));

    // The Display message must mention `links = "libz"` so an auditor
    // sees the native-FFI hint at the diagnostic layer.
    let err = BoundaryCheckError::ForbiddenBuildRs {
        violations: v,
        count: 1,
    };
    let msg = err.to_string();
    assert!(
        msg.contains("ffi_user") && msg.contains("libz"),
        "message must name crate + links: {:?}",
        msg
    );
}

/// TEST-077: in-scope crate with `[lib] proc-macro = true` fires.
#[test]
fn in_scope_proc_macro_fires_violation() {
    let m = fixture(
        vec![pkg_with_targets(
            "macro_user",
            "path+file:///m#0.1.0",
            vec![target("macro_user", &["proc-macro"])],
            None,
        )],
        vec!["path+file:///m#0.1.0"],
        vec![],
    );
    let v = find_proc_macro_violations(&["macro_user".into()], &m).unwrap();
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].crate_name, "macro_user");
}

/// TEST-078 (a): out-of-scope crate with build.rs does NOT fire.
#[test]
fn out_of_scope_build_rs_does_not_fire() {
    let m = fixture(
        vec![
            pkg_with_targets(
                "in_scope",
                "path+file:///s#0.1.0",
                vec![target("in_scope", &["lib"])],
                None,
            ),
            pkg_with_targets(
                "external_helper",
                "path+file:///x#0.1.0",
                vec![
                    target("external_helper", &["lib"]),
                    target("build-script-build", &["custom-build"]),
                ],
                None,
            ),
        ],
        vec!["path+file:///s#0.1.0", "path+file:///x#0.1.0"],
        vec![],
    );
    let v = find_build_rs_violations(&["in_scope".into()], &m).unwrap();
    assert!(v.is_empty(), "out-of-scope build.rs must not fire: {:?}", v);
}

/// TEST-078 (b): out-of-scope crate with proc-macro target does NOT fire.
#[test]
fn out_of_scope_proc_macro_does_not_fire() {
    let m = fixture(
        vec![
            pkg_with_targets(
                "in_scope",
                "path+file:///s#0.1.0",
                vec![target("in_scope", &["lib"])],
                None,
            ),
            pkg_with_targets(
                "external_macro",
                "path+file:///x#0.1.0",
                vec![target("external_macro", &["proc-macro"])],
                None,
            ),
        ],
        vec!["path+file:///s#0.1.0", "path+file:///x#0.1.0"],
        vec![],
    );
    let v = find_proc_macro_violations(&["in_scope".into()], &m).unwrap();
    assert!(
        v.is_empty(),
        "out-of-scope proc-macro must not fire: {:?}",
        v
    );
}

// ============================================================================
// LLR-073 / TEST-080: DAL-A MC/DC qualification gate
// ============================================================================

use crate::policy::{AuxiliaryMcdcTool, Dal};
use std::collections::BTreeMap;

fn aux_tool() -> AuxiliaryMcdcTool {
    AuxiliaryMcdcTool {
        name: "LDRA TBvision".into(),
        qualification_id: Some("TQL-1-LDRA-001".into()),
        report: Some("auxiliary/mcdc.json".into()),
    }
}

#[test]
fn dal_a_mcdc_check_passes_when_no_dal_a_in_scope() {
    let mut dal_map = BTreeMap::new();
    dal_map.insert("crate_b".into(), Dal::B);
    dal_map.insert("crate_d".into(), Dal::D);
    assert!(check_dal_a_mcdc_evidence(&dal_map, None).is_ok());
}

#[test]
fn dal_a_mcdc_check_passes_when_auxiliary_tool_set() {
    let mut dal_map = BTreeMap::new();
    dal_map.insert("crate_a1".into(), Dal::A);
    dal_map.insert("crate_a2".into(), Dal::A);
    let tool = aux_tool();
    assert!(check_dal_a_mcdc_evidence(&dal_map, Some(&tool)).is_ok());
}

#[test]
fn dal_a_mcdc_check_fires_on_dal_a_without_tool() {
    let mut dal_map = BTreeMap::new();
    // Insert in non-sorted order to verify the error sorts offenders.
    dal_map.insert("zeta_crate".into(), Dal::A);
    dal_map.insert("alpha_crate".into(), Dal::A);
    dal_map.insert("dal_b_crate".into(), Dal::B);
    let err = check_dal_a_mcdc_evidence(&dal_map, None).unwrap_err();
    match err {
        BoundaryCheckError::DalAMissingAuxiliaryMcdc {
            dal_a_crates,
            count,
        } => {
            assert_eq!(count, 2);
            assert_eq!(dal_a_crates, vec!["alpha_crate", "zeta_crate"]);
        }
        other => panic!("wrong variant: {:?}", other),
    }
}

#[test]
fn dal_a_mcdc_error_message_lists_crates_and_cites_upstream() {
    let mut dal_map = BTreeMap::new();
    dal_map.insert("flight_core".into(), Dal::A);
    let err = check_dal_a_mcdc_evidence(&dal_map, None).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("flight_core"),
        "message must name the offender crate: {:?}",
        msg
    );
    assert!(
        msg.contains("DAL-A"),
        "message must name the DAL: {:?}",
        msg
    );
}
