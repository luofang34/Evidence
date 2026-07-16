//! Verify-time replay of the boundary build.rs / proc-macro policy.
//!
//! Pulled out of the parent `bundle.rs` so the orchestrator stays
//! under the workspace 500-line limit.

use std::fs;
use std::path::Path;

use crate::bundle::EvidenceIndex;

use super::super::errors::VerifyError;

/// LLR-072: when `index.boundary_policy` claims `forbid_build_rs`
/// or `forbid_proc_macros` enforcement, replay those rules at
/// verify time against the bundle’s `cargo_metadata.json`
/// projection. Closes the window where a build.rs / proc-macro
/// added between generate and verify would otherwise pass
/// verification silently.
///
/// Scoping uses `index.dal_map.keys()` as the in-scope set
/// — the same set the dal_map was derived from at generate
/// time, so the recheck honors per-crate scoping the same way
/// generate did. A bundle whose `dal_map` is empty (legacy or
/// pre-DAL bundle) skips the recheck even if the policy claims
/// it; without scoping data the recheck would either fire on
/// every workspace member or no one, neither of which is
/// useful.
pub(super) fn check_boundary_recheck(
    bundle: &Path,
    index: &EvidenceIndex,
    errors: &mut Vec<VerifyError>,
) {
    let policy = &index.boundary_policy;
    if !policy.forbid_build_rs && !policy.forbid_proc_macros {
        return;
    }
    if index.dal_map.is_empty() {
        return;
    }
    let metadata_path = bundle.join("cargo_metadata.json");
    if !metadata_path.is_file() {
        errors.push(VerifyError::BoundaryVerifyMetadataMissing);
        return;
    }
    let raw = match fs::read_to_string(&metadata_path) {
        Ok(s) => s,
        Err(_) => {
            errors.push(VerifyError::BoundaryVerifyMetadataMissing);
            return;
        }
    };
    let projection =
        match crate::cargo_metadata::CargoMetadataProjection::from_projection_json(&raw) {
            Ok(p) => p,
            Err(_) => {
                errors.push(VerifyError::BoundaryVerifyMetadataMissing);
                return;
            }
        };
    let in_scope: Vec<String> = index.dal_map.keys().cloned().collect();
    if policy.forbid_build_rs {
        let v = crate::cargo_metadata::check_build_rs_in_projection(&in_scope, &projection);
        if !v.is_empty() {
            let details = v
                .iter()
                .map(|x| match &x.links {
                    Some(l) => format!("{} (links = \"{}\")", x.crate_name, l),
                    None => x.crate_name.clone(),
                })
                .collect::<Vec<_>>()
                .join(", ");
            errors.push(VerifyError::BoundaryVerifyForbiddenBuildRs { details });
        }
    }
    if policy.forbid_proc_macros {
        let v = crate::cargo_metadata::check_proc_macros_in_projection(&in_scope, &projection);
        if !v.is_empty() {
            let details = v
                .iter()
                .map(|x| x.crate_name.clone())
                .collect::<Vec<_>>()
                .join(", ");
            errors.push(VerifyError::BoundaryVerifyForbiddenProcMacro { details });
        }
    }
}
