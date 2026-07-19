//! Corpus-native record file schemas.
//!
//! Native requirement records project identity, layer, title, and
//! decomposition edges into the graph. Record descriptions remain
//! file metadata; `schema_version` gates the accepted shape.

use std::path::Path;

use serde::Deserialize;
use uuid::{Uuid, Variant, Version};

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
        reason = "record metadata is outside the graph identity projection"
    )]
    description: Option<String>,
}

/// Parse the native requirement file at `path` and insert its records
/// into `graph`.
///
/// # Errors
///
/// Fails closed on unreadable/malformed input, a newer
/// `schema_version`, a uid outside the `req_<UUIDv4>` scheme, or a
/// graph identity collision.
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
        validate_requirement_uid(&record.uid)?;
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

fn validate_requirement_uid(uid: &str) -> Result<(), CorpusError> {
    let suffix =
        uid.strip_prefix(REQUIREMENT_UID_PREFIX)
            .ok_or_else(|| CorpusError::NativeUidPrefix {
                uid: uid.to_string(),
                expected: REQUIREMENT_UID_PREFIX,
            })?;
    let valid_v4 = Uuid::parse_str(suffix).is_ok_and(|parsed| {
        parsed.get_version() == Some(Version::Random) && parsed.get_variant() == Variant::RFC4122
    });
    if !valid_v4 {
        return Err(CorpusError::NativeUidUuidV4 {
            uid: uid.to_string(),
        });
    }
    Ok(())
}
