//! Corpus-native record file schemas.
//!
//! Native requirement records project identity, layer, title,
//! normative content (description, rationale, scope, category,
//! source, verification methods), and decomposition edges into the
//! graph (LLR-113). `schema_version` gates the accepted shape.

use std::path::Path;

use serde::Deserialize;
use uuid::{Uuid, Variant, Version};

use super::error::CorpusError;
use super::graph::{
    CorpusGraph, EdgeKind, Node, RequirementLayer, RequirementNode, canonical_strings,
};

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

/// One native requirement record. The optional fields are the
/// review-sensitive normative content the graph retains (LLR-113).
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
    description: Option<String>,
    #[serde(default)]
    rationale: Option<String>,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    verification_methods: Vec<String>,
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
            description: record.description,
            rationale: record.rationale,
            scope: record.scope,
            category: record.category,
            source: record.source,
            verification_methods: canonical_strings(&record.verification_methods),
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

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
mod tests {
    use super::*;

    fn load_single_record(content: &str) -> Result<CorpusGraph, CorpusError> {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("records.toml");
        std::fs::write(&path, content).expect("write records");
        let mut graph = CorpusGraph::new();
        load_requirements_into(&path, &mut graph)?;
        Ok(graph)
    }

    /// A newer `schema_version` still fails closed — the loader
    /// refuses rather than silently skipping unknown structure.
    #[test]
    fn records_refuse_newer_schema() {
        let err = load_single_record("schema_version = 999\n")
            .expect_err("newer schema must fail closed");
        assert!(
            matches!(err, CorpusError::RecordSchemaTooNew { found: 999, .. }),
            "expected RecordSchemaTooNew, got: {err:?}"
        );
    }

    /// The record's review-sensitive content fields land on the node
    /// in canonical form (LLR-113).
    #[test]
    fn records_project_review_content_fields() {
        let graph = load_single_record(
            r#"
schema_version = 1

[[requirements]]
uid = "req_00000000-0000-4000-8000-00000000000a"
id = "R-A"
layer = "hlr"
title = "content carrier"
description = "Normative prose."
rationale = "Why it exists."
scope = "component"
category = "functional"
source = "SRS-1"
verification_methods = ["test", "review", "test"]
"#,
        )
        .expect("load record");

        let Some(Node::Requirement(node)) = graph.get("req_00000000-0000-4000-8000-00000000000a")
        else {
            panic!("requirement node missing");
        };
        assert_eq!(node.description.as_deref(), Some("Normative prose."));
        assert_eq!(node.rationale.as_deref(), Some("Why it exists."));
        assert_eq!(node.scope.as_deref(), Some("component"));
        assert_eq!(node.category.as_deref(), Some("functional"));
        assert_eq!(node.source.as_deref(), Some("SRS-1"));
        assert_eq!(
            node.verification_methods,
            vec!["review".to_string(), "test".to_string()],
            "set-like lists load sorted and duplicate-free"
        );
    }
}
