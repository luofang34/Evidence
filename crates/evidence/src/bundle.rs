//! Evidence bundle creation and management.
//!
//! Split across sibling files under `bundle/`:
//!
//! | Sub-module        | Concern                                                   |
//! |-------------------|-----------------------------------------------------------|
//! | [`command`]       | `CommandRecord` — rows in `commands.json`                 |
//! | [`test_summary`]  | `TestSummary` + `parse_cargo_test_output`                 |
//! | [`capture`]       | `normalize_captured_text` — LF-normalize stdout/stderr    |
//! | [`signing`]       | `sign_bundle` / `verify_bundle_signature` (HMAC envelope) |
//! | [`index`]         | `EvidenceIndex` — struct mirror of `index.json`           |
//! | [`builder`]       | `EvidenceBuildConfig`, `EvidenceBuilder` (assembly state) |
//! | [`time`]          | `utc_now_rfc3339` + `utc_compact_stamp`                   |
//!
//! Re-exports below keep the crate's public API flat — every
//! consumer continues to `use evidence::bundle::{EvidenceBuilder, …}`
//! without caring about the split.

pub mod builder;
pub mod capture;
pub mod command;
pub mod error;
pub mod index;
pub mod signing;
pub mod test_summary;
pub mod time;

pub use builder::{EvidenceBuildConfig, EvidenceBuilder};
pub use command::CommandRecord;
pub use error::BuilderError;
pub use index::EvidenceIndex;
pub use signing::{SigningError, sign_bundle, verify_bundle_signature};
pub use test_summary::{TestSummary, parse_cargo_test_output};
pub use time::{utc_compact_stamp, utc_now_rfc3339};
