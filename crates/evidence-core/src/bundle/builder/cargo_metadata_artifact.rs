//! Helper that writes the `cargo_metadata.json` projection into the
//! bundle directory. Pulled out of the parent `builder.rs` so the
//! orchestrator stays under the workspace 500-line limit.

use std::fs;
use std::path::Path;

use crate::bundle::error::BuilderError;
use crate::cargo_metadata::CargoMetadataProjection;
use crate::util::cmd_stdout;

/// Shell out to `cargo metadata --format-version 1`, build a
/// [`CargoMetadataProjection`] from the raw output, and write it to
/// `cargo_metadata.json` in `bundle_dir`. Called from `finalize`
/// only when the boundary policy enables `forbid_build_rs` or
/// `forbid_proc_macros` — see LLR-072. Determinism: the projection
/// sorts packages by name, and serde's `to_string_pretty` is stable,
/// so two runs on the same input produce byte-identical artifact
/// bytes (SYS-003).
pub(super) fn write_cargo_metadata_projection(bundle_dir: &Path) -> Result<(), BuilderError> {
    let raw = cmd_stdout("cargo", &["metadata", "--format-version", "1"])
        .map_err(BuilderError::CargoMetadataRun)?;
    let projection = CargoMetadataProjection::from_raw_metadata(&raw)
        .map_err(BuilderError::CargoMetadataProject)?;
    let bytes = projection
        .to_canonical_json()
        .map_err(|source| BuilderError::Serialize {
            kind: "cargo_metadata.json",
            source,
        })?;
    let path = bundle_dir.join("cargo_metadata.json");
    fs::write(&path, bytes).map_err(|source| BuilderError::Io {
        op: "writing",
        path: path.clone(),
        source,
    })?;
    Ok(())
}
