//! Adoption-state doctor tests (LLR-107): the
//! trace-validity check's behavior when the evidence behind the
//! requested claim is missing, empty, or unconfigured. Split out
//! of the parent `doctor_cmd.rs` (via `#[path]`) to stay under the
//! 500-line workspace file-size limit.
//!
//! - `dal_a_empty_trace_fires_doctor_trace_empty` — claim-grade
//!   DAL fails closed on a zero-requirement tree.
//! - `dal_d_empty_trace_warns_no_evidence` — dev mode reports the
//!   explicit warning-severity adoption diagnostic, exit stays 0.
//! - `dal_d_missing_trace_root_warns_not_adopted` — configured-but-
//!   absent roots get their own warning code.

use std::fs;

use tempfile::TempDir;

use super::helpers::run_doctor;

/// **DAL-A empty-trace silent-pass gate.**
/// `validate_trace_links_with_policy` on an empty-everything tree
/// is trivially valid (no HLR to iterate → DAL-A's
/// `require_hlr_sys_trace` has nothing to fail on), so a plain
/// validator run would report `[✓] trace validity` + `DOCTOR_OK`
/// on a DAL-A project with zero trace data. The explicit DAL ≥ C
/// empty-trace gate fires
/// `DOCTOR_TRACE_EMPTY` instead, so a cert-grade target without
/// trace data cannot silently-pass.
#[test]
fn dal_a_empty_trace_fires_doctor_trace_empty() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path();

    // Populate `cert/trace/` with valid TOML but zero requirements.
    // This is the scenario commit 4 specifically catches — a
    // readable but empty trace tree, distinct from
    // `DOCTOR_TRACE_INVALID` which fires on unreadable / missing
    // roots.
    fs::create_dir_all(root.join("cert").join("trace")).unwrap();
    for (name, content) in [
        (
            "hlr.toml",
            "requirements = []\n\n[schema]\nversion = \"0.0.1\"\n\n\
             [meta]\ndocument_id = \"DS-HLR\"\nrevision = \"1.0\"\n",
        ),
        (
            "sys.toml",
            "requirements = []\n\n[schema]\nversion = \"0.0.1\"\n\n\
             [meta]\ndocument_id = \"DS-SYS\"\nrevision = \"1.0\"\n",
        ),
        (
            "llr.toml",
            "requirements = []\n\n[schema]\nversion = \"0.0.1\"\n\n\
             [meta]\ndocument_id = \"DS-LLR\"\nrevision = \"1.0\"\n",
        ),
        (
            "tests.toml",
            "tests = []\n\n[schema]\nversion = \"0.0.1\"\n\n\
             [meta]\ndocument_id = \"DS-TESTS\"\nrevision = \"1.0\"\n",
        ),
    ] {
        fs::write(root.join("cert").join("trace").join(name), content).unwrap();
    }

    // DAL-A boundary.
    fs::create_dir_all(root.join("cert")).unwrap();
    fs::write(
        root.join("cert").join("boundary.toml"),
        "[schema]\nversion = \"0.0.1\"\n\n[scope]\nin_scope = [\"downstream\"]\n\
         trace_roots = [\"cert/trace\"]\n\n[policy]\nno_out_of_scope_deps = false\n\
         forbid_build_rs = false\nforbid_proc_macros = false\n\n\
         [dal]\ndefault_dal = \"A\"\n",
    )
    .unwrap();
    fs::write(
        root.join("cert").join("floors.toml"),
        "schema_version = 1\n\n[floors]\n\n[per_crate.downstream]\n",
    )
    .unwrap();
    fs::create_dir_all(root.join(".github").join("workflows")).unwrap();
    fs::write(
        root.join(".github").join("workflows").join("ci.yml"),
        "name: CI\non: push\njobs:\n  check:\n    runs-on: ubuntu-latest\n    \
         steps:\n      - run: cargo evidence check\n",
    )
    .unwrap();
    fs::write(
        root.join("README.md"),
        "# Downstream\n\n`Override-Deterministic-Baseline: <reason>` in PR body for overrides.\n",
    )
    .unwrap();

    let (exit, diags) = run_doctor(root);
    let codes: Vec<&str> = diags.iter().map(|d| d["code"].as_str().unwrap()).collect();
    assert!(
        codes.contains(&"DOCTOR_TRACE_EMPTY"),
        "DAL-A + empty trace must fire DOCTOR_TRACE_EMPTY; codes={:?}",
        codes
    );
    assert_ne!(
        exit, 0,
        "DAL-A + empty trace must fail doctor; diags={:?}",
        diags
    );
    // The message must name the actual state — "zero requirements"
    // here, as opposed to "missing roots" — so an auditor can tell
    // adopted-but-empty apart from not-adopted (LLR-107).
    let trace_msg = diags
        .iter()
        .find(|d| d["code"].as_str() == Some("DOCTOR_TRACE_EMPTY"))
        .and_then(|d| d["message"].as_str())
        .unwrap_or("")
        .to_string();
    assert!(
        trace_msg.contains("zero requirements"),
        "DOCTOR_TRACE_EMPTY message must name the empty state; got: {}",
        trace_msg
    );
}

/// **DAL-D empty-trace adoption diagnostic (LLR-107).** Pre-fix, a
/// DAL-D project with a readable but fully-empty trace tree got
/// `[✓] trace validity` + `DOCTOR_OK` — the empty graph validated
/// vacuously and doctor reported trace validity as OK over no
/// evidence. Post-fix, development mode reports the explicit
/// non-success adoption diagnostic `DOCTOR_TRACE_NO_EVIDENCE`
/// (warning severity — iteration stays unblocked, exit stays 0)
/// instead of a silent pass.
#[test]
fn dal_d_empty_trace_warns_no_evidence() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path();

    // Readable but fully-empty trace tree.
    fs::create_dir_all(root.join("cert").join("trace")).unwrap();
    for (name, content) in [
        (
            "hlr.toml",
            "requirements = []\n\n[schema]\nversion = \"0.0.1\"\n\n\
             [meta]\ndocument_id = \"DS-HLR\"\nrevision = \"1.0\"\n",
        ),
        (
            "sys.toml",
            "requirements = []\n\n[schema]\nversion = \"0.0.1\"\n\n\
             [meta]\ndocument_id = \"DS-SYS\"\nrevision = \"1.0\"\n",
        ),
        (
            "llr.toml",
            "requirements = []\n\n[schema]\nversion = \"0.0.1\"\n\n\
             [meta]\ndocument_id = \"DS-LLR\"\nrevision = \"1.0\"\n",
        ),
        (
            "tests.toml",
            "tests = []\n\n[schema]\nversion = \"0.0.1\"\n\n\
             [meta]\ndocument_id = \"DS-TESTS\"\nrevision = \"1.0\"\n",
        ),
    ] {
        fs::write(root.join("cert").join("trace").join(name), content).unwrap();
    }

    // DAL-D boundary + the configs doctor's other checks need.
    fs::create_dir_all(root.join("cert")).unwrap();
    fs::write(
        root.join("cert").join("boundary.toml"),
        "[schema]\nversion = \"0.0.1\"\n\n[scope]\nin_scope = [\"downstream\"]\n\
         trace_roots = [\"cert/trace\"]\n\n[policy]\nno_out_of_scope_deps = false\n\
         forbid_build_rs = false\nforbid_proc_macros = false\n\n\
         [dal]\ndefault_dal = \"D\"\n",
    )
    .unwrap();
    fs::write(
        root.join("cert").join("floors.toml"),
        "schema_version = 1\n\n[floors]\n\n[per_crate.downstream]\n",
    )
    .unwrap();
    fs::create_dir_all(root.join(".github").join("workflows")).unwrap();
    fs::write(
        root.join(".github").join("workflows").join("ci.yml"),
        "name: CI\non: push\njobs:\n  check:\n    runs-on: ubuntu-latest\n    \
         steps:\n      - run: cargo evidence check\n",
    )
    .unwrap();
    fs::write(
        root.join("README.md"),
        "# Downstream\n\n`Override-Deterministic-Baseline: <reason>` in PR body for overrides.\n",
    )
    .unwrap();

    let (exit, diags) = run_doctor(root);
    let codes: Vec<&str> = diags.iter().map(|d| d["code"].as_str().unwrap()).collect();
    assert_eq!(
        exit, 0,
        "DAL-D empty trace is an adoption warning, not an error; diags={:?}",
        diags
    );
    let no_evidence = diags
        .iter()
        .find(|d| d["code"].as_str() == Some("DOCTOR_TRACE_NO_EVIDENCE"))
        .unwrap_or_else(|| {
            panic!(
                "expected DOCTOR_TRACE_NO_EVIDENCE warning row; codes={:?}",
                codes
            )
        });
    assert_eq!(
        no_evidence["severity"].as_str(),
        Some("warning"),
        "the adoption diagnostic must be warning-severity so dev exit stays 0; got {:?}",
        no_evidence
    );
    assert_eq!(
        codes.last().copied(),
        Some("DOCTOR_OK"),
        "stream must still terminate with DOCTOR_OK; got codes={:?}",
        codes
    );
}

/// **DAL-D missing-roots adoption diagnostic (LLR-107).** Configured
/// trace roots that don't exist on disk are a distinct adoption
/// state from "present but empty" and get their own warning code.
#[test]
fn dal_d_missing_trace_root_warns_not_adopted() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path();

    // boundary.toml configures cert/trace but the directory does
    // not exist — NotAdopted, not Empty.
    fs::create_dir_all(root.join("cert")).unwrap();
    fs::write(
        root.join("cert").join("boundary.toml"),
        "[schema]\nversion = \"0.0.1\"\n\n[scope]\nin_scope = [\"downstream\"]\n\
         trace_roots = [\"cert/trace\"]\n\n[policy]\nno_out_of_scope_deps = false\n\
         forbid_build_rs = false\nforbid_proc_macros = false\n\n\
         [dal]\ndefault_dal = \"D\"\n",
    )
    .unwrap();
    fs::write(
        root.join("cert").join("floors.toml"),
        "schema_version = 1\n\n[floors]\n\n[per_crate.downstream]\n",
    )
    .unwrap();
    fs::create_dir_all(root.join(".github").join("workflows")).unwrap();
    fs::write(
        root.join(".github").join("workflows").join("ci.yml"),
        "name: CI\non: push\njobs:\n  check:\n    runs-on: ubuntu-latest\n    \
         steps:\n      - run: cargo evidence check\n",
    )
    .unwrap();
    fs::write(
        root.join("README.md"),
        "# Downstream\n\n`Override-Deterministic-Baseline: <reason>` in PR body for overrides.\n",
    )
    .unwrap();

    let (exit, diags) = run_doctor(root);
    let codes: Vec<&str> = diags.iter().map(|d| d["code"].as_str().unwrap()).collect();
    assert_eq!(
        exit, 0,
        "DAL-D missing roots is an adoption warning, not an error; diags={:?}",
        diags
    );
    let not_adopted = diags
        .iter()
        .find(|d| d["code"].as_str() == Some("DOCTOR_TRACE_NOT_ADOPTED"))
        .unwrap_or_else(|| {
            panic!(
                "expected DOCTOR_TRACE_NOT_ADOPTED warning row; codes={:?}",
                codes
            )
        });
    assert_eq!(
        not_adopted["severity"].as_str(),
        Some("warning"),
        "the adoption diagnostic must be warning-severity; got {:?}",
        not_adopted
    );
    let msg = not_adopted["message"].as_str().unwrap_or("").to_string();
    assert!(
        msg.contains("cert/trace"),
        "message must name the missing root; got: {}",
        msg
    );
    assert_eq!(
        codes.last().copied(),
        Some("DOCTOR_OK"),
        "stream must still terminate with DOCTOR_OK; got codes={:?}",
        codes
    );
}
