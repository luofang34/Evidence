//! `corpus.toml` — the strict, layout-agnostic index of linked graph
//! files (HLR-079, LLR-100).
//!
//! The index names which files carry graph entries, per node kind. A
//! path entry is either a literal file or a `<dir>/**/*.toml`
//! recursive pattern; expansion is deterministic (sorted) and never
//! follows symlinks. Everything degenerate fails closed: unknown
//! fields, a newer schema, an entry resolving to nothing, and
//! unsupported node kinds. Requirement and review files load through
//! the same resolution mechanism (LLR-116).

use std::path::{Path, PathBuf};

use serde::Deserialize;
use walkdir::WalkDir;

use super::error::CorpusError;
use super::graph::CorpusGraph;
use super::records;
use super::review_records;

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
        let mut requirement_files = Vec::new();
        for entry in &file.requirements {
            requirement_files.extend(resolve_entry(root, entry)?);
        }
        let mut review_files = Vec::new();
        for entry in &file.reviews {
            review_files.extend(resolve_entry(root, entry)?);
        }
        Ok(Self {
            requirement_files,
            review_files,
        })
    }

    /// Load the full graph named by the index at `path`: resolve the
    /// indexed files, union their entries into one graph, and validate
    /// edge resolution. Requirement files load before review files,
    /// so review records validate against present requirement nodes
    /// and requirement errors surface first (LLR-116).
    pub fn load_graph(path: &Path) -> Result<CorpusGraph, CorpusError> {
        let index = Self::load(path)?;
        let mut graph = CorpusGraph::new();
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
    let unsupported: [(&'static str, &[String]); 6] = [
        ("sources", &file.sources),
        ("source_graphs", &file.source_graphs),
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

// Tests live in a sibling file pulled in via `#[path]` so this
// facade stays under the 500-line workspace limit.
#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
#[path = "index/tests.rs"]
mod tests;
