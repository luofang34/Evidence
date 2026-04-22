//! `check_qualification` — cert-grade DAL gate on
//! `cert/QUALIFICATION.md`. Carved out of
//! `cli/doctor/checks.rs` to keep the parent under the 500-line
//! workspace limit.

use std::path::Path;

use evidence_core::policy::Dal;

use super::CheckResult;
use super::checks::load_default_dal;

/// `cert/QUALIFICATION.md` gate for cert-grade DAL targets.
///
/// Fires `DOCTOR_QUALIFICATION_MISSING` (Error) at DAL ≥ C when
/// the file is absent. DAL-D projects skip the check — the
/// qualification gap statement is advisory below DAL-C; no
/// cert-grade auditor reads it. Mirrors `check_trace`'s
/// DAL-gated-Error pattern. Follow-up: add a Warning-severity
/// advisory code for DAL-D if downstream projects ask for it.
pub(super) fn check_qualification(workspace: &Path) -> CheckResult {
    let (dal, fallback_note) = load_default_dal(workspace);
    if dal < Dal::C {
        return CheckResult::Pass;
    }
    let qual_path = workspace.join("cert").join("QUALIFICATION.md");
    if qual_path.is_file() {
        return CheckResult::Pass;
    }
    CheckResult::Fail(
        "DOCTOR_QUALIFICATION_MISSING",
        format!(
            "cert/QUALIFICATION.md missing at DAL-{:?}{} — cert-grade DAL \
             requires a tool-qualification document (A-7 Obj-7 gap statement, \
             MC/DC stance, DO-330 TQL boundary). Run `cargo evidence init` to \
             scaffold the template or copy the upstream cert/QUALIFICATION.md \
             into this workspace.",
            dal, fallback_note
        ),
    )
}
