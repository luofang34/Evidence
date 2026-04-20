//! Single source of truth for on-disk schema version strings.
//!
//! Every `schema_version` / `[schema].version` field that references
//! one of the four on-disk formats this tool writes must flow from a
//! constant here, never a string literal at the use site. A grep
//! regression test (`tests/schema_versions_locked.rs` in the
//! `evidence` crate) walks the source tree and fails if any
//! `"0.0.[0-9]"`-shaped string survives outside this module.
//!
//! # Why
//!
//! When versions are repeated as literals across `bundle.rs`,
//! `compliance.rs`, the CLI init templates, and test fixtures, a
//! schema bump requires archaeology — grep until you've found every
//! site, pray you haven't missed one. Centralizing here means a bump
//! is exactly two edits: change the constant, regenerate the golden
//! fixture in the same PR.
//!
//! # Compatibility
//!
//! Bumping any of these is a schema-breaking change by definition.
//! The on-disk format is not covered by semver today (see README's
//! Project Status section — pre-1.0). A future release will tie
//! these constants to `cargo semver-checks` or an equivalent gate.

/// Schema version for `index.json`. Covers the EvidenceIndex shape.
pub const INDEX: &str = "0.0.1";

/// Schema version for `boundary.toml` under the `[schema]` table.
pub const BOUNDARY: &str = "0.0.1";

/// Schema version for `cert/trace/*.toml` (HLR, LLR, tests, derived).
/// All four trace files share one version because they deserialize
/// through the same struct family.
///
/// **Pre-ship policy**: every constant in this module is pinned at
/// `"0.0.1"` until the project ships a 1.0. Breaking changes rewrite
/// rule text in place; they do **not** bump the version. Enforced by
/// `schema_constants_pinned_at_001` in
/// `tests/schema_versions_locked.rs`. If you find yourself wanting
/// to bump, either (a) your change is a genuine 1.0 ship — pin to
/// a non-`"0.0.1"` value there and drop the pin test, or (b) your
/// change is still pre-ship — rewrite in place and update the
/// regression test for the drifted shape without touching the
/// version string.
pub const TRACE: &str = "0.0.1";

/// Schema version for per-crate `compliance/*.json` reports.
pub const COMPLIANCE: &str = "0.0.1";

/// Schema version for `deterministic-manifest.json` — the committed,
/// hashed projection of `env.json` whose hash is recorded as
/// `index.json.deterministic_hash`.
pub const DETERMINISTIC_MANIFEST: &str = "0.0.1";

/// Schema version for the `--format=jsonl` diagnostic wire format.
///
/// Unlike the other constants here, this does **not** go into any
/// on-disk bundle file — it's the version of the streaming JSON-Lines
/// contract that the CLI emits on stdout for agent consumers.
/// Committed under `schemas/diagnostic.schema.json`. Bumping means a
/// breaking change to the 10 Schema Rules documented in
/// [`crate::diagnostic`].
pub const DIAGNOSTIC: &str = "0.0.1";
