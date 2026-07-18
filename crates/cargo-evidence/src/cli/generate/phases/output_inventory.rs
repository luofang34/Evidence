//! Phase 5b — inventory + hash the workspace's compiled deliverables.
//!
//! Reads the build's `compiler-artifact` messages, hashes each in-scope
//! deliverable, and records them into `outputs_hashes.json`. The
//! inventory runs its own `cargo build --message-format=json`, so it is
//! independent of the test phase — a full generate always inventories, a
//! `--skip-tests` generate can opt in via `--inventory-outputs`, and a
//! strict (cert/record) generate always inventories.

use anyhow::Result;

use evidence_core::{EvidenceBuilder, Profile, inventory_outputs_blocking};

/// Whether the inventory runs. It does its own
/// `cargo build --message-format=json`, so it is independent of the test
/// phase: a full generate always inventories; a `--skip-tests` generate
/// inventories when `--inventory-outputs` opts in; and a strict
/// (cert/record) generate always inventories, so a cert bundle attests
/// its deliverables even if the caller omitted the flag.
fn should_inventory(skip_tests: bool, inventory_outputs: bool, strict: bool) -> bool {
    !skip_tests || inventory_outputs || strict
}

/// Inventory the workspace's compiled deliverables via
/// `cargo build --message-format=json` and hash each into
/// `outputs_hashes.json`. Short-circuits when [`should_inventory`] is
/// false (a plain `--skip-tests` dev bundle records no deliverables
/// unless `--inventory-outputs` is set). Strict (cert/record) mode
/// inventories unconditionally and fails closed if the build produced no
/// in-scope deliverables, or if a captured artifact cannot be hashed.
pub(in crate::cli::generate) fn inventory_and_hash_outputs(
    builder: &mut EvidenceBuilder,
    profile: Profile,
    skip_tests: bool,
    inventory_outputs: bool,
    strict: bool,
    quiet: bool,
    json_output: bool,
) -> Result<()> {
    if !should_inventory(skip_tests, inventory_outputs, strict) {
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

#[cfg(test)]
mod tests {
    use super::should_inventory;

    #[test]
    fn full_generate_always_inventories() {
        assert!(should_inventory(false, false, false));
        assert!(should_inventory(false, true, false));
    }

    #[test]
    fn skip_tests_inventories_only_when_opted_in() {
        assert!(!should_inventory(true, false, false));
        assert!(should_inventory(true, true, false));
    }

    #[test]
    fn strict_inventories_even_without_the_flag() {
        // cert/record must attest its deliverables even if the caller
        // forgot `--inventory-outputs` — an empty-output cert bundle
        // would otherwise verify successfully.
        assert!(should_inventory(true, false, true));
    }
}
