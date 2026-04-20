//! env.json vs index.json cross-file consistency check.
//!
//! If env.json is unreadable or unparseable, the check is skipped —
//! the SHA256SUMS hash check (step 5 in the orchestrator) catches
//! corruption of env.json anyway. Only the fields that logically
//! must agree between the two files are cross-checked here.

use std::path::Path;

use crate::bundle::EvidenceIndex;

use super::errors::VerifyError;

/// Compare env.json-level fields against the same fields in index.json
/// and push a [`VerifyError::CrossFileInconsistency`] for each one that
/// disagrees.
pub(super) fn check_env_vs_index(
    bundle: &Path,
    index: &EvidenceIndex,
    errors: &mut Vec<VerifyError>,
) {
    let env_path = bundle.join("env.json");
    let env_content = match std::fs::read_to_string(&env_path) {
        Ok(s) => s,
        Err(_) => return,
    };
    let env_value: serde_json::Value = match serde_json::from_str(&env_content) {
        Ok(v) => v,
        Err(_) => return,
    };

    if let Some(env_profile) = env_value.get("profile").and_then(|v| v.as_str()) {
        // `index.profile` is typed `Profile`; compare via its
        // canonical on-wire string (`"dev"` / `"cert"` / `"record"`).
        let index_profile_str = index.profile.to_string();
        if env_profile != index_profile_str {
            errors.push(VerifyError::CrossFileInconsistency {
                field: "profile".to_string(),
                index_value: index_profile_str,
                env_value: env_profile.to_string(),
            });
        }
    }
    if let Some(env_git_sha) = env_value.get("git_sha").and_then(|v| v.as_str()) {
        if env_git_sha != index.git_sha {
            errors.push(VerifyError::CrossFileInconsistency {
                field: "git_sha".to_string(),
                index_value: index.git_sha.clone(),
                env_value: env_git_sha.to_string(),
            });
        }
    }
    if let Some(env_branch) = env_value.get("git_branch").and_then(|v| v.as_str()) {
        if env_branch != index.git_branch {
            errors.push(VerifyError::CrossFileInconsistency {
                field: "git_branch".to_string(),
                index_value: index.git_branch.clone(),
                env_value: env_branch.to_string(),
            });
        }
    }
    if let Some(env_dirty) = env_value.get("git_dirty").and_then(|v| v.as_bool()) {
        if env_dirty != index.git_dirty {
            errors.push(VerifyError::CrossFileInconsistency {
                field: "git_dirty".to_string(),
                index_value: index.git_dirty.to_string(),
                env_value: env_dirty.to_string(),
            });
        }
    }
}
