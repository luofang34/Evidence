//! Unit tests for `evidence_core::cargo_metadata`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use super::*;

fn raw_fixture() -> String {
    serde_json::json!({
        "packages": [
            {
                "name": "z_proc",
                "id": "path+file:///z#0.1.0",
                "targets": [{"kind": ["proc-macro"]}],
                "links": null
            },
            {
                "name": "a_lib",
                "id": "path+file:///a#0.1.0",
                "targets": [{"kind": ["lib"]}]
            },
            {
                "name": "m_ffi",
                "id": "path+file:///m#0.1.0",
                "targets": [
                    {"kind": ["lib"]},
                    {"kind": ["custom-build"]}
                ],
                "links": "libz"
            }
        ],
        "workspace_members": [],
        "resolve": {"nodes": []}
    })
    .to_string()
}

/// TEST-079 (a): the projection sorts packages by name and
/// retains targets[].kind + links — re-running on the same input
/// yields byte-identical JSON.
#[test]
fn projection_is_deterministic() {
    let raw = raw_fixture();
    let p1 = CargoMetadataProjection::from_raw_metadata(&raw).unwrap();
    let p2 = CargoMetadataProjection::from_raw_metadata(&raw).unwrap();
    let j1 = p1.to_canonical_json().unwrap();
    let j2 = p2.to_canonical_json().unwrap();
    assert_eq!(j1, j2);

    // Sort order: a_lib < m_ffi < z_proc.
    let names: Vec<&str> = p1.packages.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, vec!["a_lib", "m_ffi", "z_proc"]);

    // m_ffi retains its links value.
    let m = p1.packages.iter().find(|p| p.name == "m_ffi").unwrap();
    assert_eq!(m.links.as_deref(), Some("libz"));

    // m_ffi has both lib + custom-build targets.
    let kinds: Vec<&str> = m
        .targets
        .iter()
        .flat_map(|t| t.kind.iter().map(String::as_str))
        .collect();
    assert!(kinds.contains(&"custom-build"));
    assert!(kinds.contains(&"lib"));
}

/// TEST-079 (b): write then re-read — the projection round-trips
/// without losing any field that the recheck depends on.
#[test]
fn projection_round_trip() {
    let raw = raw_fixture();
    let original = CargoMetadataProjection::from_raw_metadata(&raw).unwrap();
    let json = original.to_canonical_json().unwrap();
    let reread = CargoMetadataProjection::from_projection_json(&json).unwrap();
    assert_eq!(original, reread);
}

#[test]
fn check_build_rs_in_projection_fires_on_in_scope_only() {
    let raw = raw_fixture();
    let proj = CargoMetadataProjection::from_raw_metadata(&raw).unwrap();

    let v_in_scope = check_build_rs_in_projection(&["m_ffi".into()], &proj);
    assert_eq!(v_in_scope.len(), 1);
    assert_eq!(v_in_scope[0].crate_name, "m_ffi");
    assert_eq!(v_in_scope[0].links.as_deref(), Some("libz"));

    let v_oos = check_build_rs_in_projection(&["a_lib".into()], &proj);
    assert!(v_oos.is_empty());
}

#[test]
fn check_proc_macros_in_projection_fires_on_in_scope_only() {
    let raw = raw_fixture();
    let proj = CargoMetadataProjection::from_raw_metadata(&raw).unwrap();

    let v_in_scope = check_proc_macros_in_projection(&["z_proc".into()], &proj);
    assert_eq!(v_in_scope.len(), 1);
    assert_eq!(v_in_scope[0].crate_name, "z_proc");

    let v_oos = check_proc_macros_in_projection(&["a_lib".into()], &proj);
    assert!(v_oos.is_empty());
}

#[test]
fn projection_parse_failure_returns_typed_error() {
    let bad = "not json at all";
    let err = CargoMetadataProjection::from_raw_metadata(bad).unwrap_err();
    assert!(matches!(err, ProjectionError::ParseRawMetadata(_)));
}
