//! TEST-060: `load_max_dal` derives trace-policy DAL as
//! `max(dal_map.values())` across per-crate overrides, not just
//! `default_dal`. Observable via `DOCTOR_QUALIFICATION_MISSING`
//! — fires at DAL ≥ C when `cert/QUALIFICATION.md` is absent.
//! A project at `default_dal = "D"` that overrides one crate to
//! DAL-A must fire the qualification gate; pre-fix
//! (`load_default_dal`) silently suppressed it because only
//! `default_dal` fed the policy.
//!
//! Normal / robustness / BVA triplet per DO-178C DAL-A/B
//! verification expectations.
//!
//! Scenarios:
//! - **Normal**: default=D + one crate overridden to A → fires.
//!   The overridden-up crate must raise the effective DAL.
//! - **Robustness (baseline)**: default=D + no overrides →
//!   does NOT fire. Control for the normal case; proves the
//!   observable is gated on DAL, not on boundary.toml presence.
//! - **Robustness (empty in_scope)**: default=A + in_scope=[]
//!   → fires. `dal_map()` is empty; fallback is
//!   `cfg.dal.default_dal`, not a hard `Dal::D` floor.
//! - **BVA (multi-level mix)**: default=D + one crate at B +
//!   one crate at A → fires. The `max()` across dal_map picks
//!   A; same observable as the single-override case but proves
//!   `max` handles N ≥ 2 overrides.
//! - **BVA (at-threshold)**: default=D + one crate at C →
//!   fires. `DAL == C` must pass the `>= C` gate (strict
//!   equality, not strict-greater).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use std::fs;
use std::path::Path;

use assert_cmd::Command;
use evidence_core::schema_versions;
use serde_json::Value;
use tempfile::TempDir;

const BOUNDARY_V: &str = schema_versions::BOUNDARY;

fn cargo_evidence() -> Command {
    #[allow(deprecated)]
    Command::cargo_bin("cargo-evidence").unwrap()
}

fn run_doctor(workspace: &Path) -> Vec<Value> {
    let out = cargo_evidence()
        .args(["evidence", "doctor", "--format=jsonl"])
        .current_dir(workspace)
        .output()
        .expect("spawn cargo-evidence");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("each line must be valid JSON"))
        .collect()
}

/// Build a workspace whose only DAL-sensitive doctor observable
/// is the QUALIFICATION.md gate. No QUALIFICATION.md is
/// written; its presence depends on the caller's DAL scenario.
fn setup(default_dal: &str, in_scope: &[&str], overrides: &[(&str, &str)]) -> TempDir {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path();

    fs::create_dir_all(root.join("cert")).unwrap();
    let in_scope_list = in_scope
        .iter()
        .map(|s| format!("\"{s}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let overrides_toml = if overrides.is_empty() {
        String::new()
    } else {
        let entries = overrides
            .iter()
            .map(|(k, v)| format!("{k} = \"{v}\""))
            .collect::<Vec<_>>()
            .join("\n");
        format!("\n[dal.crate_overrides]\n{entries}\n")
    };
    fs::write(
        root.join("cert").join("boundary.toml"),
        format!(
            r#"[schema]
version = "{BOUNDARY_V}"

[scope]
in_scope = [{in_scope_list}]

[policy]
no_out_of_scope_deps = false

[dal]
default_dal = "{default_dal}"
{overrides_toml}
"#
        ),
    )
    .unwrap();

    tmp
}

fn qualification_missing_fired(diags: &[Value]) -> bool {
    diags
        .iter()
        .any(|d| d.get("code").and_then(|c| c.as_str()) == Some("DOCTOR_QUALIFICATION_MISSING"))
}

/// **Normal.** default=D + single crate override to A. Pre-fix:
/// `load_default_dal` reads "D" → gate skipped. Post-fix:
/// `load_max_dal` computes max over dal_map → A → gate fires.
#[test]
fn override_raises_dal_past_qualification_gate() {
    let tmp = setup("D", &["payment_gateway"], &[("payment_gateway", "A")]);
    let diags = run_doctor(tmp.path());
    assert!(
        qualification_missing_fired(&diags),
        "override to DAL-A must raise effective DAL past the ≥C \
         qualification gate; got diagnostics: {:?}",
        diags
            .iter()
            .filter_map(|d| d.get("code").and_then(|c| c.as_str()))
            .collect::<Vec<_>>()
    );
}

/// **Robustness baseline.** default=D + no overrides. Effective
/// DAL = D, below the ≥C qualification gate. This case is the
/// control for the normal test: if the observable fires here,
/// we're not really measuring what we think we're measuring.
#[test]
fn no_override_at_default_d_leaves_gate_skipped() {
    let tmp = setup("D", &["crate_x"], &[]);
    let diags = run_doctor(tmp.path());
    assert!(
        !qualification_missing_fired(&diags),
        "DAL-D must not fire the qualification gate; control for \
         the override-raises test. Got: {:?}",
        diags
            .iter()
            .filter_map(|d| d.get("code").and_then(|c| c.as_str()))
            .collect::<Vec<_>>()
    );
}

/// **Robustness (empty in_scope).** `default_dal = "A"` +
/// `in_scope = []`. `dal_map()` is empty → `max()` returns
/// `None`. The fallback must honor the declared `default_dal`,
/// not drop to a hard `Dal::D` floor — a pre-scoping config
/// with `in_scope = []` and `default_dal = "A"` is a legitimate
/// state.
#[test]
fn empty_in_scope_falls_back_to_default_dal_not_hard_d() {
    let tmp = setup("A", &[], &[]);
    let diags = run_doctor(tmp.path());
    assert!(
        qualification_missing_fired(&diags),
        "empty in_scope must fall back to default_dal=A, not \
         Dal::D — pre-scoping projects shouldn't silently \
         downgrade. Got: {:?}",
        diags
            .iter()
            .filter_map(|d| d.get("code").and_then(|c| c.as_str()))
            .collect::<Vec<_>>()
    );
}

/// **BVA (multi-level mix).** Two crates overridden to
/// different DALs (B and A); default_dal=D applies to neither.
/// `max()` must pick A. Same observable as the single-override
/// case but proves the reduction generalizes past N=1.
#[test]
fn mixed_overrides_take_highest() {
    let tmp = setup("D", &["core", "kernel"], &[("core", "B"), ("kernel", "A")]);
    let diags = run_doctor(tmp.path());
    assert!(
        qualification_missing_fired(&diags),
        "max over (B, A) = A must fire ≥C gate; got: {:?}",
        diags
            .iter()
            .filter_map(|d| d.get("code").and_then(|c| c.as_str()))
            .collect::<Vec<_>>()
    );
}

/// **BVA (at-threshold).** Single crate at exactly DAL-C. The
/// gate is `dal >= Dal::C`, not `dal > Dal::C` — equality must
/// pass. Tests the strict boundary of the threshold.
#[test]
fn single_crate_at_dal_c_fires_gate_inclusively() {
    let tmp = setup("D", &["lib"], &[("lib", "C")]);
    let diags = run_doctor(tmp.path());
    assert!(
        qualification_missing_fired(&diags),
        "DAL-C must fire the ≥C gate (inclusive boundary); got: {:?}",
        diags
            .iter()
            .filter_map(|d| d.get("code").and_then(|c| c.as_str()))
            .collect::<Vec<_>>()
    );
}
