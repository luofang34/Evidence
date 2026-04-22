//! Integration tests for `cargo evidence doctor` (TEST-048).
//!
//! Covers the four selectors TEST-048 pins:
//! - `rigorous_fixture_passes` — synthetic fixture with trace +
//!   floors + boundary + ci.yml + README override section passes
//!   with `DOCTOR_OK` + 6 `DOCTOR_CHECK_PASSED`.
//! - `sloppy_fixture_fails_with_named_codes` — fixture missing
//!   trace + floors + boundary fires with specific fail codes and
//!   the `DOCTOR_FAIL` terminal.
//! - `cert_generate_blocks_on_doctor_fail` — `generate --profile
//!   cert` on a sloppy fixture aborts before bundle assembly,
//!   surfacing the doctor codes in the error message.
//! - `current_workspace_passes_doctor` — our own repo passes all
//!   6 checks (load-bearing regression: if the tool's own rigor
//!   slips, this fires).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use std::fs;

use tempfile::TempDir;

#[path = "doctor_cmd/helpers.rs"]
mod helpers;

use helpers::{cargo_evidence, run_doctor, setup_rigorous_fixture, workspace_root};

#[test]
fn rigorous_fixture_passes() {
    let tmp = setup_rigorous_fixture();
    let (exit, diags) = run_doctor(tmp.path());
    assert_eq!(exit, 0, "rigorous fixture should exit 0; diags={:?}", diags);
    let codes: Vec<&str> = diags.iter().map(|d| d["code"].as_str().unwrap()).collect();
    // Exactly 8 lines: 7 checks + 1 terminal. No DOCTOR_FAIL, no
    // error-severity DOCTOR_* in the stream.
    assert_eq!(
        codes.len(),
        8,
        "expected 7 check diagnostics + 1 terminal = 8 lines; got codes={:?}",
        codes
    );
    let errors: Vec<&&str> = diags
        .iter()
        .filter(|d| d["severity"].as_str() == Some("error"))
        .zip(codes.iter())
        .map(|(_, c)| c)
        .collect();
    assert!(
        errors.is_empty(),
        "rigorous fixture produced error-severity diagnostics: {:?}",
        errors
    );
    assert_eq!(
        codes.last().copied(),
        Some("DOCTOR_OK"),
        "stream must terminate with DOCTOR_OK; got codes={:?}",
        codes
    );
    // Warnings are OK (e.g. DOCTOR_MERGE_STYLE_UNKNOWN on a
    // non-git tempdir fixture). What matters for the rigorous case
    // is that nothing fires at error severity.
}

#[test]
fn sloppy_fixture_fails_with_named_codes() {
    // Empty tempdir — no tool/trace, no cert/, no .github/, no README.
    // With DAL-D as the implicit default (boundary missing), the
    // trace check is lenient and an empty trace directory passes
    // (link-validity on zero links trivially holds). The sloppy
    // contract is now "floors + boundary missing fires" — the trace
    // check is covered separately in `downstream_dal_a_fixture_*`.
    let tmp = TempDir::new().expect("tempdir");
    let (exit, diags) = run_doctor(tmp.path());
    assert_eq!(exit, 2, "sloppy fixture should exit 2; diags={:?}", diags);
    let codes: Vec<&str> = diags.iter().map(|d| d["code"].as_str().unwrap()).collect();
    assert!(
        codes.contains(&"DOCTOR_FLOORS_MISSING"),
        "expected DOCTOR_FLOORS_MISSING in codes={:?}",
        codes
    );
    assert!(
        codes.contains(&"DOCTOR_BOUNDARY_MISSING"),
        "expected DOCTOR_BOUNDARY_MISSING in codes={:?}",
        codes
    );
    assert_eq!(
        codes.last().copied(),
        Some("DOCTOR_FAIL"),
        "stream must terminate with DOCTOR_FAIL; got codes={:?}",
        codes
    );
}

/// Downstream simulation: a DAL-D project with minimal rigor still
/// passes doctor's trace check. Specifically: HLRs have no
/// `surfaces` claims (surface bijection stays off at every DAL,
/// but this demonstrates a downstream HLR with domain-specific
/// surfaces we don't know about), and no SYS layer
/// (require_hlr_sys_trace is off at DAL-D). Without DAL-derived
/// policy this scenario would hit `DOCTOR_TRACE_INVALID` because
/// the check would hardcode all-strict flags.
#[test]
fn downstream_dal_d_fixture_passes() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path();

    // Minimal DAL-D project shape: HLR with empty surfaces, empty
    // traces_to (no SYS layer). No derived. Each entry has a real
    // UUID so register-phase validation passes.
    fs::create_dir_all(root.join("tool").join("trace")).unwrap();
    fs::write(
        root.join("tool").join("trace").join("hlr.toml"),
        "[schema]\nversion = \"0.0.1\"\n\n[meta]\ndocument_id = \"DS-HLR\"\nrevision = \"1.0\"\n\n\
         [[requirements]]\nuid = \"91d2a98f-7b89-4e3c-8d1d-4b7f8e77a9b4\"\nid = \"HLR-001\"\n\
         title = \"Downstream HLR\"\nowner = \"downstream\"\nscope = \"component\"\n\
         description = \"Downstream domain requirement\"\nverification_methods = [\"test\"]\n\
         traces_to = []\nsurfaces = []\n",
    )
    .unwrap();
    fs::write(
        root.join("tool").join("trace").join("sys.toml"),
        "requirements = []\n\n[schema]\nversion = \"0.0.1\"\n\n\
         [meta]\ndocument_id = \"DS-SYS\"\nrevision = \"1.0\"\n",
    )
    .unwrap();
    fs::write(
        root.join("tool").join("trace").join("llr.toml"),
        "requirements = []\n\n[schema]\nversion = \"0.0.1\"\n\n\
         [meta]\ndocument_id = \"DS-LLR\"\nrevision = \"1.0\"\n",
    )
    .unwrap();
    fs::write(
        root.join("tool").join("trace").join("tests.toml"),
        "tests = []\n\n[schema]\nversion = \"0.0.1\"\n\n\
         [meta]\ndocument_id = \"DS-TESTS\"\nrevision = \"1.0\"\n",
    )
    .unwrap();

    // DAL-D boundary + required configs.
    fs::create_dir_all(root.join("cert")).unwrap();
    fs::write(
        root.join("cert").join("boundary.toml"),
        "[schema]\nversion = \"0.0.1\"\n\n[scope]\nin_scope = [\"downstream\"]\n\
         trace_roots = [\"tool/trace\"]\n\n[policy]\nno_out_of_scope_deps = false\n\
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
        "DAL-D downstream fixture must pass doctor (trace bijection \
         is off at this level); diags={:?}",
        diags
    );
    let has_trace_error = diags
        .iter()
        .any(|d| d["code"].as_str() == Some("DOCTOR_TRACE_INVALID"));
    assert!(
        !has_trace_error,
        "DAL-D must NOT fire DOCTOR_TRACE_INVALID on empty-surfaces / no-SYS trace; codes={:?}",
        codes
    );
    assert_eq!(
        codes.last().copied(),
        Some("DOCTOR_OK"),
        "stream must terminate with DOCTOR_OK; got codes={:?}",
        codes
    );
}

/// DAL-A strictness: a project declaring `default_dal = "A"` with
/// an HLR lacking `traces_to` fires `DOCTOR_TRACE_INVALID` because
/// DAL-A's derived policy sets `require_hlr_sys_trace: true`. The
/// counterpart to `downstream_dal_d_fixture_passes` — proves the
/// DAL-derived policy actually tightens at higher levels.
#[test]
fn downstream_dal_a_fixture_catches_missing_sys() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path();

    // HLR with empty traces_to + valid UUID — would pass at DAL-D,
    // must fail at DAL-A via require_hlr_sys_trace.
    fs::create_dir_all(root.join("tool").join("trace")).unwrap();
    fs::write(
        root.join("tool").join("trace").join("hlr.toml"),
        "[schema]\nversion = \"0.0.1\"\n\n[meta]\ndocument_id = \"DS-HLR\"\nrevision = \"1.0\"\n\n\
         [[requirements]]\nuid = \"91d2a98f-7b89-4e3c-8d1d-4b7f8e77a9b4\"\nid = \"HLR-001\"\n\
         title = \"Orphaned HLR\"\nowner = \"downstream\"\nscope = \"component\"\n\
         description = \"Should fail DAL-A\"\nverification_methods = [\"test\"]\n\
         traces_to = []\nsurfaces = []\n",
    )
    .unwrap();
    for (name, doc, list_key) in [
        ("sys.toml", "DS-SYS", "requirements"),
        ("llr.toml", "DS-LLR", "requirements"),
        ("tests.toml", "DS-TESTS", "tests"),
    ] {
        fs::write(
            root.join("tool").join("trace").join(name),
            format!(
                "{} = []\n\n[schema]\nversion = \"0.0.1\"\n\n\
                 [meta]\ndocument_id = \"{}\"\nrevision = \"1.0\"\n",
                list_key, doc
            ),
        )
        .unwrap();
    }

    fs::create_dir_all(root.join("cert")).unwrap();
    fs::write(
        root.join("cert").join("boundary.toml"),
        "[schema]\nversion = \"0.0.1\"\n\n[scope]\nin_scope = [\"downstream\"]\n\
         trace_roots = [\"tool/trace\"]\n\n[policy]\nno_out_of_scope_deps = false\n\
         forbid_build_rs = false\nforbid_proc_macros = false\n\n\
         [dal]\ndefault_dal = \"A\"\n",
    )
    .unwrap();
    fs::write(
        root.join("cert").join("floors.toml"),
        "schema_version = 1\n\n[floors]\n\n[per_crate.downstream]\n",
    )
    .unwrap();

    let (exit, diags) = run_doctor(root);
    let codes: Vec<&str> = diags.iter().map(|d| d["code"].as_str().unwrap()).collect();
    assert_eq!(
        exit, 2,
        "DAL-A must catch the orphan HLR and fail; diags={:?}",
        diags
    );
    assert!(
        codes.contains(&"DOCTOR_TRACE_INVALID"),
        "DAL-A must fire DOCTOR_TRACE_INVALID on missing SYS trace; codes={:?}",
        codes
    );
    // Message should include the DAL level for auditor context.
    let trace_msg: String = diags
        .iter()
        .find(|d| d["code"].as_str() == Some("DOCTOR_TRACE_INVALID"))
        .and_then(|d| d["message"].as_str())
        .unwrap_or("")
        .to_string();
    assert!(
        trace_msg.contains("DAL-A") || trace_msg.contains("DAL-"),
        "trace failure message must include DAL level for context; got: {}",
        trace_msg
    );
}

#[test]
fn cert_generate_blocks_on_doctor_fail() {
    // Sloppy fixture: no trace, no floors, no boundary.
    let tmp = TempDir::new().expect("tempdir");
    // A minimal Cargo.toml so `generate`'s preflight + profile resolution
    // doesn't fail on something upstream of the doctor precheck.
    fs::write(
        tmp.path().join("Cargo.toml"),
        "[workspace]\nmembers = []\nresolver = \"2\"\n",
    )
    .unwrap();

    let out_dir = TempDir::new().expect("tempdir").keep();
    let out = cargo_evidence()
        .args(["evidence", "generate", "--profile", "cert", "--out-dir"])
        .arg(&out_dir)
        .current_dir(tmp.path())
        .output()
        .expect("spawn");
    let exit = out.status.code().unwrap_or(-1);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_ne!(
        exit, 0,
        "cert-profile generate on sloppy fixture must abort; exit={}\nstderr:\n{}\nstdout:\n{}",
        exit, stderr, stdout
    );
    // The precheck error message enumerates the triggered
    // DOCTOR_* codes via the anyhow chain.
    let combined = format!("{}{}", stderr, stdout);
    assert!(
        combined.contains("doctor precheck failed")
            || combined.contains("DOCTOR_TRACE_INVALID")
            || combined.contains("DOCTOR_FLOORS_MISSING")
            || combined.contains("DOCTOR_BOUNDARY_MISSING"),
        "stderr/stdout must surface the doctor codes; \nstderr:\n{}\nstdout:\n{}",
        stderr,
        stdout,
    );
}

/// Boundary-missing + trace-fails: the trace diagnostic's message
/// must carry the explicit fallback note so the user isn't
/// silently looking at DAL-D output when their project was
/// targeting DAL-A. Without the note, a misleading "trace
/// validation failed at DAL-D" reads as "problem fixed at DAL-D"
/// when the real problem is "boundary file broken, so we don't
/// know what level this project targets."
#[test]
fn fallback_note_appears_when_boundary_missing_and_trace_fails() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path();

    // Valid trace layout with a dangling traces_to — fails at any
    // DAL level including DAL-D's lenient default.
    fs::create_dir_all(root.join("tool").join("trace")).unwrap();
    fs::write(
        root.join("tool").join("trace").join("hlr.toml"),
        "[schema]\nversion = \"0.0.1\"\n\n[meta]\ndocument_id = \"DS-HLR\"\nrevision = \"1.0\"\n\n\
         [[requirements]]\nuid = \"91d2a98f-7b89-4e3c-8d1d-4b7f8e77a9b4\"\nid = \"HLR-001\"\n\
         title = \"Dangling HLR\"\nowner = \"downstream\"\nscope = \"component\"\n\
         description = \"Points at a ghost SYS UID\"\nverification_methods = [\"test\"]\n\
         traces_to = [\"00000000-0000-0000-0000-000000000000\"]\nsurfaces = []\n",
    )
    .unwrap();
    for (name, doc, list_key) in [
        ("sys.toml", "DS-SYS", "requirements"),
        ("llr.toml", "DS-LLR", "requirements"),
        ("tests.toml", "DS-TESTS", "tests"),
    ] {
        fs::write(
            root.join("tool").join("trace").join(name),
            format!(
                "{} = []\n\n[schema]\nversion = \"0.0.1\"\n\n\
                 [meta]\ndocument_id = \"{}\"\nrevision = \"1.0\"\n",
                list_key, doc
            ),
        )
        .unwrap();
    }
    // No cert/boundary.toml on purpose — forces load_default_dal
    // into the fallback branch (boundary_loadable = false).

    let (exit, diags) = run_doctor(root);
    assert_eq!(
        exit, 2,
        "dangling traces_to must fire at DAL-D; diags={:?}",
        diags
    );
    let trace_msg: String = diags
        .iter()
        .find(|d| d["code"].as_str() == Some("DOCTOR_TRACE_INVALID"))
        .and_then(|d| d["message"].as_str())
        .unwrap_or("")
        .to_string();
    assert!(
        trace_msg.contains("assumed DAL-D"),
        "trace failure must carry the fallback note so the DAL-D \
         assumption isn't silent; got message: {}",
        trace_msg
    );
    assert!(
        trace_msg.contains("boundary unloadable"),
        "fallback note must name the root cause (boundary unloadable); \
         got message: {}",
        trace_msg
    );
}

#[test]
fn current_workspace_passes_doctor() {
    // Load-bearing regression: if the tool's own rigor slips below
    // its published checklist (trace breaks, floors drift, README
    // drops the override section), this test fires.
    let workspace = workspace_root();
    let (exit, diags) = run_doctor(&workspace);
    assert_eq!(
        exit, 0,
        "cargo-evidence's own workspace must pass its own doctor; \
         diags={:?}",
        diags
    );
    let codes: Vec<&str> = diags.iter().map(|d| d["code"].as_str().unwrap()).collect();
    assert_eq!(
        codes.last().copied(),
        Some("DOCTOR_OK"),
        "self-dogfood must terminate with DOCTOR_OK; got codes={:?}",
        codes
    );
}

/// **DAL-A empty-trace silent-pass gate.** `check_trace` used to
/// load only `<workspace>/tool/trace`, and
/// `validate_trace_links_with_policy` on an empty-everything tree
/// is trivially valid (no HLR to iterate → DAL-A's
/// `require_hlr_sys_trace` has nothing to fail on). Result:
/// `[✓] trace validity` + `DOCTOR_OK` on a DAL-A project with zero
/// trace data. The explicit DAL ≥ C empty-trace gate fires
/// `DOCTOR_TRACE_EMPTY` instead, so a cert-grade target without
/// trace data cannot silently-pass.
#[test]
fn dal_a_empty_trace_fires_doctor_trace_empty() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path();

    // Populate `tool/trace/` with valid TOML but zero requirements.
    // This is the scenario commit 4 specifically catches — a
    // readable but empty trace tree, distinct from
    // `DOCTOR_TRACE_INVALID` which fires on unreadable / missing
    // roots.
    fs::create_dir_all(root.join("tool").join("trace")).unwrap();
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
        fs::write(root.join("tool").join("trace").join(name), content).unwrap();
    }

    // DAL-A boundary.
    fs::create_dir_all(root.join("cert")).unwrap();
    fs::write(
        root.join("cert").join("boundary.toml"),
        "[schema]\nversion = \"0.0.1\"\n\n[scope]\nin_scope = [\"downstream\"]\n\
         trace_roots = [\"tool/trace\"]\n\n[policy]\nno_out_of_scope_deps = false\n\
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
}
