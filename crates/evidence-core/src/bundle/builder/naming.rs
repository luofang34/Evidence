//! Bundle directory naming and creation.

use std::fs;
use std::path::PathBuf;

use super::EvidenceBuildConfig;
use crate::bundle::BuilderError;
use crate::git::GitSnapshot;

pub(super) fn create_bundle_directories(
    config: &EvidenceBuildConfig,
    git_snapshot: &GitSnapshot,
    timestamp: &str,
) -> Result<PathBuf, BuilderError> {
    let sha_short = git_snapshot.sha.get(..8).unwrap_or(&git_snapshot.sha);
    let bundle_dir = config
        .output_root
        .join(format!("{}-{}-{}", config.profile, timestamp, sha_short));

    if bundle_dir.exists() {
        return Err(BuilderError::BundleExists { path: bundle_dir });
    }

    create_directory(&bundle_dir)?;
    create_directory(&bundle_dir.join("tests"))?;
    create_directory(&bundle_dir.join("trace"))?;
    Ok(bundle_dir)
}

fn create_directory(path: &std::path::Path) -> Result<(), BuilderError> {
    fs::create_dir_all(path).map_err(|source| BuilderError::Io {
        op: "creating",
        path: path.to_path_buf(),
        source,
    })
}
