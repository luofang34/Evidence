//! `corpus.toml` — the strict, layout-agnostic index of linked graph
//! files (HLR-079, LLR-100).
//!
//! The index names which files carry graph entries, per node kind. A
//! path entry is either a literal file or a `<dir>/**/*.toml`
//! recursive pattern; expansion is deterministic (sorted) and never
//! follows symlinks. Everything degenerate fails closed: unknown
//! fields, a newer schema, an entry resolving to nothing, and kinds
//! this tool version cannot load yet.

use std::path::{Path, PathBuf};

use serde::Deserialize;
use walkdir::WalkDir;

use super::error::CorpusError;
use super::graph::CorpusGraph;
use super::records;

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
}

impl CorpusIndex {
    /// Parse `corpus.toml` at `path` and resolve every indexed entry
    /// to concrete files.
    ///
    /// # Errors
    ///
    /// Fails closed on unreadable/malformed input, a newer
    /// `schema_version`, an entry resolving to no files, or a
    /// non-empty kind this tool version cannot load.
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
        reject_unimplemented(&file)?;

        let root = path.parent().unwrap_or_else(|| Path::new("."));
        let mut requirement_files = Vec::new();
        for entry in &file.requirements {
            requirement_files.extend(resolve_entry(root, entry)?);
        }
        Ok(Self { requirement_files })
    }

    /// Load the full graph named by the index at `path`: resolve the
    /// indexed files, union their entries into one graph, and validate
    /// edge resolution.
    pub fn load_graph(path: &Path) -> Result<CorpusGraph, CorpusError> {
        let index = Self::load(path)?;
        let mut graph = CorpusGraph::new();
        for file in &index.requirement_files {
            records::load_requirements_into(file, &mut graph)?;
        }
        graph.validate()?;
        Ok(graph)
    }
}

/// Kinds the index may declare but this tool version cannot load.
/// Listing entries for one is an error — refusing beats silently
/// ignoring indexed data (HLR-079).
fn reject_unimplemented(file: &IndexFile) -> Result<(), CorpusError> {
    let unimplemented: [(&'static str, &[String]); 7] = [
        ("sources", &file.sources),
        ("source_graphs", &file.source_graphs),
        ("ambiguities", &file.ambiguities),
        ("decisions", &file.decisions),
        ("profiles", &file.profiles),
        ("reviews", &file.reviews),
        ("tests", &file.tests),
    ];
    for (kind, entries) in unimplemented {
        if !entries.is_empty() {
            return Err(CorpusError::UnimplementedKind { kind });
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
