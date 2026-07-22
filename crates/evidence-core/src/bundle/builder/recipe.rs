//! Helper that assembles [`RecipeInputs`] for the recipe manifest
//! from the builder's recorded state. Pulled out of the parent
//! `builder.rs` so the orchestrator stays under the workspace
//! 500-line limit.

use std::collections::BTreeMap;

use crate::bundle::command::CommandRecord;
use crate::bundle::error::BuilderError;
use crate::cargo_metadata::CargoMetadataProjection;
use crate::env::{
    RecipeInputs, RecipeProjectionError, commands_digest, inputs_digest, locked_graph_digest,
};
use crate::policy::ResolutionPolicy;

/// Assemble the recipe inputs for the manifest from the recorded
/// build state: digests over the canonical serializations of the
/// input and command state (byte-identical to what
/// [`super::EvidenceBuilder::write_inputs`] /
/// [`super::EvidenceBuilder::write_commands`] persist, so the
/// generate-time and verify-time digests agree), the locked-graph
/// digest when a `cargo_metadata.json` projection was written, an
/// empty feature list (the tool does not set cargo features), and
/// the configured resolution policy (LLR-144).
pub(super) fn assemble_recipe_inputs(
    inputs: &BTreeMap<String, String>,
    commands: &[CommandRecord],
    metadata_projection: Option<&CargoMetadataProjection>,
    resolution_policy: ResolutionPolicy,
) -> Result<RecipeInputs, BuilderError> {
    let locked_graph_hash = metadata_projection
        .map(locked_graph_digest)
        .transpose()
        .map_err(recipe_projection_err)?;
    Ok(RecipeInputs {
        features: Vec::new(),
        locked_graph_hash,
        command_recipe_hash: commands_digest(commands).map_err(recipe_projection_err)?,
        inputs_hash: inputs_digest(inputs).map_err(recipe_projection_err)?,
        resolution_policy,
    })
}

/// Fold a projection error into the builder's error type. `Parse`
/// cannot arise from these three digest helpers (they serialize
/// in-memory state, never parse), but the conversion stays
/// exhaustive rather than panicking: it degrades to the same
/// serialize-failure variant the caller would have produced.
fn recipe_projection_err(e: RecipeProjectionError) -> BuilderError {
    match e {
        RecipeProjectionError::Serialize { kind, source }
        | RecipeProjectionError::Parse { kind, source, .. } => {
            BuilderError::Serialize { kind, source }
        }
        RecipeProjectionError::Io { op, path, source } => BuilderError::Io { op, path, source },
    }
}
