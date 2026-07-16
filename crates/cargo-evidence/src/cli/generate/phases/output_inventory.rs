//! Phase 5b — inventory + hash the workspace's compiled deliverables.
//!
//! Split out of the parent `phases` module so it stays under the
//! 500-line workspace file-size limit.

use anyhow::Result;

use evidence_core::{EvidenceBuilder, Profile, inventory_outputs_blocking};

/// Inventory the workspace's compiled deliverables via
/// `cargo build --message-format=json` and hash each into
/// `outputs_hashes.json`. Skipped when tests are skipped (a
/// `--skip-tests` bundle compiles nothing, so it has no build outputs
/// to attest). Strict (cert/record) mode fails closed if the build
/// produced no in-scope deliverables, or if a captured artifact cannot
/// be hashed.
pub(in crate::cli::generate) fn inventory_and_hash_outputs(
    builder: &mut EvidenceBuilder,
    profile: Profile,
    skip_tests: bool,
    strict: bool,
    quiet: bool,
    json_output: bool,
) -> Result<()> {
    if skip_tests {
        return Ok(());
    }
    match inventory_outputs_blocking(profile) {
        Ok(artifacts) => {
            if artifacts.is_empty() && strict {
                return Err(anyhow::anyhow!(
                    "output inventory captured zero deliverables; refusing to record an \
                     empty output manifest for a cert/record bundle"
                ));
            }
            for art in &artifacts {
                if let Err(e) = builder.add_output(art.key.clone(), &art.path) {
                    if strict {
                        return Err(anyhow::Error::new(e)
                            .context(format!("hashing output artifact: {}", art.key)));
                    }
                    tracing::warn!("could not hash output {}: {}", art.key, e);
                }
            }
            if !quiet && !json_output {
                println!("evidence: hashed {} build output(s)", artifacts.len());
            }
        }
        Err(e) => {
            if strict {
                return Err(anyhow::Error::new(e).context("inventorying build outputs"));
            }
            tracing::warn!("could not inventory build outputs: {}", e);
        }
    }
    Ok(())
}
