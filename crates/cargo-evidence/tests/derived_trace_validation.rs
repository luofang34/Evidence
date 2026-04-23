//! TEST-055: derived.toml requirements participate in
//! `cargo evidence trace --validate`'s Link-phase checks.
//!
//! Pre-fix, `cli/trace.rs:120` passed `&[]` as the derived-
//! entry slice. Consequence: the `require_derived_rationale`
//! policy (DAL-C+) never ran — a derived entry with no
//! rationale silently passed validation. Post-fix, the real
//! `derived.requirements` vector threads through the validator
//! and the policy check fires.
//!
//! Two cases assert the fix:
//! - positive: DAL-A workspace with derived rationale present
//!   → validation passes (`VERIFY_OK`).
//! - negative: DAL-A workspace with derived rationale missing
//!   → validation fails with `TRACE_DERIVED_MISSING_RATIONALE`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use std::fs;

use assert_cmd::Command;
use tempfile::TempDir;

fn cargo_evidence() -> Command {
    #[allow(deprecated)]
    Command::cargo_bin("cargo-evidence").unwrap()
}

const SCHEMA_V: &str = evidence_core::schema_versions::TRACE;
const BOUNDARY_V: &str = evidence_core::schema_versions::BOUNDARY;

/// Seed `cert/trace/` with a minimal SYS/HLR/LLR/TEST chain +
/// one derived entry. `derived_rationale` controls whether the
/// derived entry carries a `rationale = "..."` field.
fn seed_workspace(tmp: &std::path::Path, derived_rationale: Option<&str>) {
    let trace_dir = tmp.join("cert").join("trace");
    fs::create_dir_all(&trace_dir).expect("mkdir cert/trace");

    // Deterministic UUIDs for assertion stability.
    let sys_uid = "11111111-1111-4111-8111-111111111111";
    let hlr_uid = "22222222-2222-4222-8222-222222222222";
    let llr_uid = "33333333-3333-4333-8333-333333333333";
    let test_uid = "44444444-4444-4444-8444-444444444444";
    let derived_uid = "55555555-5555-4555-8555-555555555555";

    // boundary.toml pins DAL-A so `require_derived_rationale`
    // is active via `EvidencePolicy::for_dal(A).trace`. A
    // non-empty `in_scope` is needed because cmd_trace computes
    // `dal = dal_map().values().max()` — empty in_scope gives
    // empty map gives Dal::D fallback regardless of default_dal.
    fs::write(
        tmp.join("cert").join("boundary.toml"),
        format!(
            r#"[schema]
version = "{BOUNDARY_V}"

[scope]
in_scope = ["fixture-crate"]

[policy]
no_out_of_scope_deps = false

[dal]
default_dal = "A"
"#
        ),
    )
    .unwrap();

    fs::write(
        trace_dir.join("sys.toml"),
        format!(
            r#"[schema]
version = "{SCHEMA_V}"

[meta]
document_id = "SYS"
revision = "1"

[[requirements]]
uid = "{sys_uid}"
id = "sys-example"
title = "System requirement"
owner = "team@example.com"
verification_methods = ["test"]
traces_to = []
"#
        ),
    )
    .unwrap();
    fs::write(
        trace_dir.join("hlr.toml"),
        format!(
            r#"[schema]
version = "{SCHEMA_V}"

[meta]
document_id = "HLR"
revision = "1"

[[requirements]]
uid = "{hlr_uid}"
id = "hlr-example"
title = "HLR"
owner = "team@example.com"
verification_methods = ["test"]
traces_to = ["{sys_uid}"]
"#
        ),
    )
    .unwrap();
    fs::write(
        trace_dir.join("llr.toml"),
        format!(
            r#"[schema]
version = "{SCHEMA_V}"

[meta]
document_id = "LLR"
revision = "1"

[[requirements]]
uid = "{llr_uid}"
id = "llr-example"
title = "LLR"
owner = "team@example.com"
verification_methods = ["test"]
traces_to = ["{hlr_uid}"]
"#
        ),
    )
    .unwrap();
    fs::write(
        trace_dir.join("tests.toml"),
        format!(
            r#"[schema]
version = "{SCHEMA_V}"

[meta]
document_id = "TEST"
revision = "1"

[[tests]]
uid = "{test_uid}"
id = "test-example"
title = "Test"
owner = "team@example.com"
traces_to = ["{llr_uid}"]
"#
        ),
    )
    .unwrap();
    let rationale_line = match derived_rationale {
        Some(s) => format!("rationale = \"{s}\"\n"),
        None => String::new(),
    };
    fs::write(
        trace_dir.join("derived.toml"),
        format!(
            r#"[schema]
version = "{SCHEMA_V}"

[meta]
document_id = "DRQ"
revision = "1"

[[requirements]]
uid = "{derived_uid}"
id = "derived-example"
title = "Derived requirement"
owner = "team@example.com"
{rationale_line}safety_impact = "none"
"#
        ),
    )
    .unwrap();
}

/// Positive case: derived entry carries rationale → validation
/// clean. Pre-fix this also passed (vacuously — derived was
/// never checked); post-fix it passes because the rationale is
/// present. Both outcomes match, but the test anchors the
/// happy-path shape for the negative test below to diff against.
#[test]
fn derived_uids_resolve_in_trace_validation() {
    let tmp = TempDir::new().expect("tempdir");
    seed_workspace(
        tmp.path(),
        Some("Needed to close a gap the HLR doesn't cover."),
    );

    let out = cargo_evidence()
        .current_dir(tmp.path())
        .args(["evidence", "trace", "--validate", "--format=jsonl"])
        .output()
        .expect("spawn");

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        out.status.code(),
        Some(0),
        "validate must succeed with rationale present; stdout:\n{stdout}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        stdout.contains("VERIFY_OK"),
        "expected VERIFY_OK terminal; stdout:\n{stdout}"
    );
    assert!(
        !stdout.contains("TRACE_DERIVED_MISSING_RATIONALE"),
        "rationale present — policy must not fire; stdout:\n{stdout}"
    );
}

/// Negative case: derived entry without rationale at DAL-A
/// → `TRACE_DERIVED_MISSING_RATIONALE`. Pre-fix this passed
/// silently because the derived slice was `&[]` and the
/// policy-loop body never ran. The assertion proves the fix
/// actually wires derived entries into the validator.
#[test]
fn derived_missing_rationale_fires_at_dal_a() {
    let tmp = TempDir::new().expect("tempdir");
    seed_workspace(tmp.path(), None);

    let out = cargo_evidence()
        .current_dir(tmp.path())
        .args(["evidence", "trace", "--validate", "--format=jsonl"])
        .output()
        .expect("spawn");

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_ne!(
        out.status.code(),
        Some(0),
        "validate must fail at DAL-A when derived entry lacks rationale; stdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("TRACE_DERIVED_MISSING_RATIONALE"),
        "expected TRACE_DERIVED_MISSING_RATIONALE; stdout:\n{stdout}"
    );
}
