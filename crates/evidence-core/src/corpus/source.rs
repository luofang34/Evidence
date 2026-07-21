//! Corpus-native frozen source-revision records (LLR-125).
//!
//! One frozen revision of a source document is a strict corpus
//! graph node — a source revision — grouped with the document's
//! other revisions under a stable `document_key` lineage key. The
//! record schema is strict and versioned; the typed material state
//! keeps available content (digest plus capture mode) distinct
//! from unavailable material (a reason and no invented digest), so
//! an unavailable revision is valid graph state but can never be
//! reported as byte-verified. Source-revision nodes carry no edges
//! at this layer, and file layout and record order are
//! non-semantic.
//!
//! Module map:
//!
//! - `records` — strict `SourceFile`/`SourceRevisionRecord` serde
//!   schema, record validation, and the `load_sources_into` loader
//! - `error` — the flat [`SourceError`] taxonomy every source
//!   failure reports through

pub(super) mod error;
pub(super) mod records;

/// Typed uid prefix for corpus-native source-revision records
/// (LLR-125).
pub const SOURCE_UID_PREFIX: &str = "src_";

// Tests live in sibling files pulled in via `#[path]`: shared
// fixtures plus one module per TEST entry.
#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
#[path = "source/tests.rs"]
mod tests;
#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
#[path = "source/tests_graph/tests.rs"]
mod tests_graph;
#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
#[path = "source/tests_support.rs"]
mod tests_support;
