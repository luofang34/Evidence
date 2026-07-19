//! `floors::count_trace_per_layer` reads the same trace location as
//! every other `cargo evidence` verb (via
//! `evidence_core::trace::default_trace_roots`). A project that places
//! traces under the canonical `cert/trace/` must see floor counts that
//! match what the loader actually parsed.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use std::fs;
use std::path::Path;

use evidence_core::floors::count_trace_per_layer;
use evidence_core::schema_versions::TRACE;
use tempfile::TempDir;

fn write_minimal_trace(trace_dir: &Path) {
    fs::create_dir_all(trace_dir).unwrap();

    fs::write(
        trace_dir.join("sys.toml"),
        format!(
            r#"
[meta]
document_id = "SYS-001"
revision = "1.0"

[schema]
version = "{TRACE}"

[[requirements]]
id = "SYS-1"
title = "System requirement under test"
owner = "soi"
uid = "11111111-1111-4111-8111-111111111111"
verification_methods = ["test"]

[[requirements]]
id = "SYS-2"
title = "Second system requirement under test"
owner = "soi"
uid = "22222222-2222-4222-8222-222222222222"
verification_methods = ["test"]
"#
        ),
    )
    .unwrap();

    fs::write(
        trace_dir.join("hlr.toml"),
        format!(
            r#"
[meta]
document_id = "HLR-001"
revision = "1.0"

[schema]
version = "{TRACE}"

[[requirements]]
id = "HLR-1"
title = "Test requirement"
owner = "soi"
uid = "33333333-3333-4333-8333-333333333333"
traces_to = ["11111111-1111-4111-8111-111111111111"]
verification_methods = ["test"]

[[requirements]]
id = "HLR-2"
title = "Second test requirement"
owner = "soi"
uid = "44444444-4444-4444-8444-444444444444"
traces_to = ["22222222-2222-4222-8222-222222222222"]
verification_methods = ["test"]
"#
        ),
    )
    .unwrap();

    fs::write(
        trace_dir.join("llr.toml"),
        format!(
            r#"
[meta]
document_id = "LLR-001"
revision = "1.0"

[schema]
version = "{TRACE}"

[[requirements]]
id = "LLR-1"
title = "LLR test"
owner = "soi"
uid = "55555555-5555-4555-8555-555555555555"
derived = false
traces_to = [
    "33333333-3333-4333-8333-333333333333",
    "44444444-4444-4444-8444-444444444444",
]
verification_methods = ["test"]

[[requirements]]
id = "LLR-2"
title = "Second LLR test"
owner = "soi"
uid = "66666666-6666-4666-8666-666666666666"
derived = false
traces_to = ["44444444-4444-4444-8444-444444444444"]
verification_methods = ["test"]
"#
        ),
    )
    .unwrap();

    fs::write(
        trace_dir.join("tests.toml"),
        format!(
            r#"
[meta]
document_id = "TESTS-001"
revision = "1.0"

[schema]
version = "{TRACE}"

[[tests]]
id = "TEST-1"
title = "Verify LLR-1"
owner = "soi"
uid = "77777777-7777-4777-8777-777777777777"
traces_to = [
    "55555555-5555-4555-8555-555555555555",
    "66666666-6666-4666-8666-666666666666",
]
test_selector = "fixture::test_one"

[[tests]]
id = "TEST-2"
title = "Verify LLR-2"
owner = "soi"
uid = "88888888-8888-4888-8888-888888888888"
traces_to = ["66666666-6666-4666-8666-666666666666"]
test_selector = "fixture::test_two"
"#
        ),
    )
    .unwrap();
}

fn reverse_record_and_edge_order_blocking(trace_dir: &Path) {
    for (file_name, records_key) in [
        ("sys.toml", "requirements"),
        ("hlr.toml", "requirements"),
        ("llr.toml", "requirements"),
        ("tests.toml", "tests"),
    ] {
        let path = trace_dir.join(file_name);
        let text = fs::read_to_string(&path).expect("read trace fixture");
        let mut document: toml::Value = toml::from_str(&text).expect("parse trace fixture");
        let records = document
            .get_mut(records_key)
            .and_then(toml::Value::as_array_mut)
            .expect("record array");
        records.reverse();
        for record in records {
            if let Some(edges) = record
                .get_mut("traces_to")
                .and_then(toml::Value::as_array_mut)
            {
                edges.reverse();
            }
        }
        fs::write(
            &path,
            toml::to_string(&document).expect("serialize fixture"),
        )
        .expect("write reordered fixture");
    }
}

fn duplicate_hlr_identity_blocking(trace_dir: &Path, field: &str) {
    let path = trace_dir.join("hlr.toml");
    let text = fs::read_to_string(&path).expect("read HLR fixture");
    let mut document: toml::Value = toml::from_str(&text).expect("parse HLR fixture");
    let requirements = document
        .get_mut("requirements")
        .and_then(toml::Value::as_array_mut)
        .expect("requirements array");
    let duplicate = requirements[0]
        .get(field)
        .and_then(toml::Value::as_str)
        .expect("identity field")
        .to_string();
    requirements[1][field] = toml::Value::String(duplicate);
    fs::write(
        &path,
        toml::to_string(&document).expect("serialize fixture"),
    )
    .expect("write duplicate fixture");
}

#[test]
fn count_trace_per_layer_finds_cert_trace_layout() {
    let tmp = TempDir::new().unwrap();
    let trace_dir = tmp.path().join("cert").join("trace");
    write_minimal_trace(&trace_dir);

    let (sys, hlr, llr, tests) = count_trace_per_layer(tmp.path());
    assert_eq!((sys, hlr, llr, tests), (2, 2, 2, 2));
}

#[test]
fn count_trace_per_layer_returns_zero_on_missing_workspace() {
    let tmp = TempDir::new().unwrap();
    let (sys, hlr, llr, tests) = count_trace_per_layer(tmp.path());
    assert_eq!((sys, hlr, llr, tests), (0, 0, 0, 0));
}

#[test]
fn graph_derived_counts_ignore_record_and_edge_input_order() {
    let tmp = TempDir::new().unwrap();
    let trace_dir = tmp.path().join("cert").join("trace");
    write_minimal_trace(&trace_dir);
    let canonical = count_trace_per_layer(tmp.path());

    reverse_record_and_edge_order_blocking(&trace_dir);

    assert_eq!(count_trace_per_layer(tmp.path()), canonical);
}

#[test]
fn graph_derived_counts_reject_duplicate_identities() {
    for field in ["uid", "id"] {
        let tmp = TempDir::new().unwrap();
        let trace_dir = tmp.path().join("cert").join("trace");
        write_minimal_trace(&trace_dir);
        duplicate_hlr_identity_blocking(&trace_dir, field);

        assert_eq!(
            count_trace_per_layer(tmp.path()),
            (0, 0, 0, 0),
            "duplicate {field} must invalidate graph-derived counts"
        );
    }
}

/// `boundary.toml` `scope.trace_roots` fallback: when the canonical
/// `cert/trace/` is absent, the discovery chain reads
/// `cert/boundary.toml` and resolves its declared roots against
/// `workspace_root`. A project with split traces (e.g.
/// `requirements/system/`, `requirements/software/`) gets per-layer
/// counts summed across them.
#[test]
fn count_trace_per_layer_reads_boundary_scope_trace_roots() {
    let tmp = TempDir::new().unwrap();
    let custom_dir = tmp.path().join("requirements");
    write_minimal_trace(&custom_dir);

    fs::create_dir_all(tmp.path().join("cert")).unwrap();
    fs::write(
        tmp.path().join("cert/boundary.toml"),
        format!(
            r#"
[schema]
version = "{ver}"

[scope]
in_scope = []
trace_roots = ["requirements"]

[policy]
no_out_of_scope_deps = false
forbid_build_rs = false
forbid_proc_macros = false
"#,
            ver = evidence_core::schema_versions::BOUNDARY
        ),
    )
    .unwrap();

    let (sys, hlr, llr, tests) = count_trace_per_layer(tmp.path());
    assert_eq!((sys, hlr, llr, tests), (2, 2, 2, 2));
}
