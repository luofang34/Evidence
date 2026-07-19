//! Corpus-native record file schemas.
//!
//! The structural core of a native requirement record: identity, layer,
//! title, and decomposition edges. Later milestones extend this shape
//! (lifecycle state and reviews, then source bindings and modality);
//! the file-level `schema_version` gates that growth.

use std::path::Path;

use serde::Deserialize;

use super::error::CorpusError;
use super::graph::{CorpusGraph, EdgeKind, Node, RequirementLayer, RequirementNode};

/// Highest requirement-record schema version this tool loads.
const SUPPORTED_RECORDS_SCHEMA: u32 = 1;

/// Typed uid prefix for corpus-native requirement records (HLR-080).
const REQUIREMENT_UID_PREFIX: &str = "req_";

/// On-disk shape of a native requirement record file. Strict: unknown
/// fields are a parse error.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RequirementFile {
    schema_version: u32,
    #[serde(default)]
    requirements: Vec<RequirementRecord>,
}

/// One native requirement record.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RequirementRecord {
    uid: String,
    id: String,
    layer: RequirementLayer,
    title: String,
    #[serde(default)]
    derives_from: Vec<String>,
    #[serde(default)]
    #[allow(
        dead_code,
        reason = "carried for later milestones; not yet a graph field"
    )]
    description: Option<String>,
}

/// Parse the native requirement file at `path` and insert its records
/// into `graph`.
///
/// # Errors
///
/// Fails closed on unreadable/malformed input, a newer
/// `schema_version`, a uid without the `req_` prefix, or a uid
/// collision in the graph.
pub(super) fn load_requirements_into(
    path: &Path,
    graph: &mut CorpusGraph,
) -> Result<(), CorpusError> {
    let raw = std::fs::read_to_string(path).map_err(|source| CorpusError::RecordRead {
        path: path.to_path_buf(),
        source,
    })?;
    let file: RequirementFile =
        toml::from_str(&raw).map_err(|source| CorpusError::RecordParse {
            path: path.to_path_buf(),
            source,
        })?;
    if file.schema_version > SUPPORTED_RECORDS_SCHEMA {
        return Err(CorpusError::RecordSchemaTooNew {
            path: path.to_path_buf(),
            found: file.schema_version,
            supported: SUPPORTED_RECORDS_SCHEMA,
        });
    }
    for record in file.requirements {
        if !record.uid.starts_with(REQUIREMENT_UID_PREFIX) {
            return Err(CorpusError::NativeUidPrefix {
                uid: record.uid,
                expected: REQUIREMENT_UID_PREFIX,
            });
        }
        let edges = record
            .derives_from
            .into_iter()
            .map(|target| (EdgeKind::DerivesFrom, target))
            .collect();
        graph.insert(Node::Requirement(RequirementNode {
            uid: record.uid,
            id: record.id,
            title: record.title,
            layer: record.layer,
            edges,
        }))?;
    }
    Ok(())
}
