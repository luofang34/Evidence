//! `corpus.toml` — the strict, layout-agnostic index of linked graph
//! files (HLR-079, LLR-100).
//!
//! The index names which files carry graph entries, per node kind. A
//! path entry is either a literal file or a `<dir>/**/*.toml`
//! recursive pattern; expansion is deterministic (sorted) and never
//! follows symlinks. Everything degenerate fails closed: unknown
//! fields, a newer schema, an entry resolving to nothing, and
//! unsupported node kinds. Requirement and review files load through
//! the same resolution mechanism (LLR-116), the reserved
//! `sources` list activates the same mechanism for source-revision
//! files (LLR-127), `source_graphs` activates it for
//! structural source-graph files (LLR-159), and `source_patches`
//! activates it for curated-patch files (LLR-169).

use std::path::{Path, PathBuf};

use serde::Deserialize;
use walkdir::WalkDir;

use super::error::CorpusError;
use super::graph::CorpusGraph;
use super::records;
use super::review_records;
use super::source;
use super::source_graph;
use super::source_patch;

/// Highest `corpus.toml` schema version this tool loads.
pub const SUPPORTED_INDEX_SCHEMA: u32 = 1;

/// On-disk shape of `corpus.toml`. Strict: unknown fields are a parse
/// error, so a typo'd kind name cannot silently drop a file list.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct IndexFile {
    schema_version: u32,
    #[serde(default)]
    sources: Vec<String>,
    #[serde(default)]
    source_graphs: Vec<String>,
    #[serde(default)]
    source_patches: Vec<String>,
    #[serde(default)]
    requirements: Vec<String>,
    #[serde(default)]
    ambiguities: Vec<String>,
    #[serde(default)]
    decisions: Vec<String>,
    #[serde(default)]
    profiles: Vec<String>,
    #[serde(default)]
    reviews: Vec<String>,
    #[serde(default)]
    tests: Vec<String>,
}

/// A parsed and resolved corpus index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorpusIndex {
    /// Resolved source-revision record files, in deterministic
    /// order (LLR-127).
    pub source_files: Vec<PathBuf>,
    /// Resolved structural source-graph record files, in
    /// deterministic order (LLR-159).
    pub source_graph_files: Vec<PathBuf>,
    /// Resolved curated-patch record files, in deterministic
    /// order (LLR-169).
    pub source_patch_files: Vec<PathBuf>,
    /// Resolved requirement record files, in deterministic order.
    pub requirement_files: Vec<PathBuf>,
    /// Resolved review record files, in deterministic order
    /// (LLR-116).
    pub review_files: Vec<PathBuf>,
}

impl CorpusIndex {
    /// Parse `corpus.toml` at `path` and resolve every indexed entry
    /// to concrete files.
    ///
    /// # Errors
    ///
    /// Fails closed on unreadable/malformed input, a newer
    /// `schema_version`, an entry resolving to no files, or a
    /// non-empty unsupported node kind.
    pub fn load(path: &Path) -> Result<Self, CorpusError> {
        let raw = std::fs::read_to_string(path).map_err(|source| CorpusError::IndexRead {
            path: path.to_path_buf(),
            source,
        })?;
        let file: IndexFile = toml::from_str(&raw).map_err(|source| CorpusError::IndexParse {
            path: path.to_path_buf(),
            source,
        })?;
        if file.schema_version > SUPPORTED_INDEX_SCHEMA {
            return Err(CorpusError::IndexSchemaTooNew {
                path: path.to_path_buf(),
                found: file.schema_version,
                supported: SUPPORTED_INDEX_SCHEMA,
            });
        }
        reject_unsupported(&file)?;

        let root = path.parent().unwrap_or_else(|| Path::new("."));
        let mut source_files = Vec::new();
        for entry in &file.sources {
            source_files.extend(resolve_entry(root, entry)?);
        }
        let mut source_graph_files = Vec::new();
        for entry in &file.source_graphs {
            source_graph_files.extend(resolve_entry(root, entry)?);
        }
        let mut source_patch_files = Vec::new();
        for entry in &file.source_patches {
            source_patch_files.extend(resolve_entry(root, entry)?);
        }
        let mut requirement_files = Vec::new();
        for entry in &file.requirements {
            requirement_files.extend(resolve_entry(root, entry)?);
        }
        let mut review_files = Vec::new();
        for entry in &file.reviews {
            review_files.extend(resolve_entry(root, entry)?);
        }
        Ok(Self {
            source_files,
            source_graph_files,
            source_patch_files,
            requirement_files,
            review_files,
        })
    }

    /// Load the full graph named by the index at `path`: resolve the
    /// indexed files, union their entries into one graph, and validate
    /// edge resolution. Source files load first (LLR-127), then
    /// source-graph files — so revision binding and media agreement
    /// validate against present revisions (LLR-159) — then
    /// curated-patch files, so patch bindings and targets validate
    /// against present revisions and parser graphs (LLR-169) — then
    /// requirement files, then review files (LLR-116), so review
    /// records validate against present requirement nodes and source
    /// and requirement errors surface first.
    pub fn load_graph(path: &Path) -> Result<CorpusGraph, CorpusError> {
        let index = Self::load(path)?;
        let mut graph = CorpusGraph::new();
        for file in &index.source_files {
            source::records::load_sources_into(file, &mut graph)?;
        }
        for file in &index.source_graph_files {
            source_graph::records::load_source_graphs_into(file, &mut graph)?;
        }
        for file in &index.source_patch_files {
            source_patch::records::load_source_patches_into(file, &mut graph).map_err(Box::new)?;
        }
        for file in &index.requirement_files {
            records::load_requirements_into(file, &mut graph)?;
        }
        for file in &index.review_files {
            review_records::load_reviews_into(file, &mut graph)?;
        }
        graph.validate()?;
        Ok(graph)
    }
}

/// Kinds declared by the index schema without a supported record
/// loader. Listing entries for one is an error (HLR-079).
fn reject_unsupported(file: &IndexFile) -> Result<(), CorpusError> {
    let unsupported: [(&'static str, &[String]); 4] = [
        ("ambiguities", &file.ambiguities),
        ("decisions", &file.decisions),
        ("profiles", &file.profiles),
        ("tests", &file.tests),
    ];
    for (kind, entries) in unsupported {
        if !entries.is_empty() {
            return Err(CorpusError::UnsupportedKind { kind });
        }
    }
    Ok(())
}

/// Resolve one index entry relative to the index's directory: a
/// `<dir>/**/*.toml` pattern walks the directory (no symlinks, sorted
/// output); anything else is a literal file path. Zero resolved files
/// is an error naming the entry.
fn resolve_entry(root: &Path, entry: &str) -> Result<Vec<PathBuf>, CorpusError> {
    if let Some(dir) = entry.strip_suffix("/**/*.toml") {
        let dir = root.join(dir);
        if !dir.is_dir() {
            return Err(CorpusError::EmptyIndexEntry {
                entry: entry.to_string(),
            });
        }
        let mut files = Vec::new();
        for walked in WalkDir::new(&dir).follow_links(false) {
            let walked = walked.map_err(|source| CorpusError::PatternWalk {
                dir: dir.clone(),
                source,
            })?;
            if walked.file_type().is_file()
                && walked.path().extension().is_some_and(|e| e == "toml")
            {
                files.push(walked.into_path());
            }
        }
        files.sort();
        if files.is_empty() {
            return Err(CorpusError::EmptyIndexEntry {
                entry: entry.to_string(),
            });
        }
        Ok(files)
    } else {
        let path = root.join(entry);
        if !path.is_file() {
            return Err(CorpusError::EmptyIndexEntry {
                entry: entry.to_string(),
            });
        }
        Ok(vec![path])
    }
}

// Index resolution and load-order tests live in a sibling file
// pulled in via `#[path]`.
#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
#[path = "index/tests.rs"]
mod tests;
