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
uid = "00000000-0000-0000-0000-000000000001"
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
uid = "00000000-0000-0000-0000-000000000010"
traces_to = ["00000000-0000-0000-0000-000000000001"]
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
uid = "00000000-0000-0000-0000-000000000020"
derived = false
traces_to = ["00000000-0000-0000-0000-000000000010"]
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
uid = "00000000-0000-0000-0000-000000000030"
traces_to = ["00000000-0000-0000-0000-000000000020"]
test_selector = "fixture::test_one"
"#
        ),
    )
    .unwrap();
}

#[test]
fn count_trace_per_layer_finds_cert_trace_layout() {
    let tmp = TempDir::new().unwrap();
    let trace_dir = tmp.path().join("cert").join("trace");
    write_minimal_trace(&trace_dir);

    let (sys, hlr, llr, tests) = count_trace_per_layer(tmp.path());
    assert_eq!((sys, hlr, llr, tests), (1, 1, 1, 1));
}

#[test]
fn count_trace_per_layer_returns_zero_on_missing_workspace() {
    let tmp = TempDir::new().unwrap();
    let (sys, hlr, llr, tests) = count_trace_per_layer(tmp.path());
    assert_eq!((sys, hlr, llr, tests), (0, 0, 0, 0));
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
    assert_eq!((sys, hlr, llr, tests), (1, 1, 1, 1));
}
