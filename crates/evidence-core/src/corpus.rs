//! Typed corpus graph over linked TOML files — the v0.2 data model.
//!
//! Certification artifacts load from files indexed by `cert/corpus.toml`
//! into one uid-keyed, typed graph; traceability reports are derived
//! views of that graph, and file layout carries no semantic meaning
//! (SYS-035). Legacy `cert/trace` documents load into the same graph
//! through [`legacy`] at exact parity until the corpus cutover.
//!
//! Module map:
//!
//! - [`index`] — `corpus.toml` parsing + per-kind file resolution
//! - [`graph`] — node/edge types and the uid-keyed graph
//! - [`records`] — corpus-native record file schemas
//! - [`legacy`] — four-file `cert/trace` → graph adapter
//!
//! Design record:
//! `docs/superpowers/specs/2026-07-18-corpus-model-v0.2-design.md`.

mod error;
mod graph;
mod index;
mod legacy;
mod records;

pub use error::CorpusError;
pub use graph::{
    CorpusGraph, EdgeKind, Node, NodeKind, RequirementLayer, RequirementNode, TestNode,
};
pub use index::{CorpusIndex, SUPPORTED_INDEX_SCHEMA};
pub use legacy::graph_from_trace_files;

#[cfg(test)]
mod tests;
