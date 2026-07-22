//! `compare_bundles` — category-complete comparison of two evidence
//! bundles (SYS-049 / HLR-113 / LLR-147).
//!
//! Where [`verify::compare_reproduction`](crate::verify::compare_reproduction)
//! answers "did THAT bundle reproduce the baseline's inputs, recipe,
//! and outputs?", this module answers the wider audit question "what
//! differs between these two bundles across EVERY assurance-relevant
//! category?" — the comparison behind `cargo evidence diff`. Each
//! category reports exactly one [`DiffCategoryStatus`] with
//! deterministic, sorted, human-readable detail lines.
//!
//! # Categories, in fixed report order
//!
//! | Category | Content compared |
//! |----------|------------------|
//! | `scope` | index fields: profile, schema versions, git identity, `dal_map`, boundary + resolution policy |
//! | `trace_graph` | `trace/*.toml` entries by uid (added / removed / changed) + `trace/matrix.md` presence |
//! | `tests` | `index.test_summary`, per-test outcome rows, captured-log presence |
//! | `coverage` | `coverage_summary.json` aggregates per measurement + `lcov.info` presence |
//! | `commands` | `commands.json` rows: argv + exit code + captured-output presence |
//! | `recipe` | `deterministic-manifest.json` recipe fields (the reproduced-output field set) |
//! | `inputs` | `inputs_hashes.json` digest plane |
//! | `outputs` | `outputs_hashes.json` digest plane |
//! | `objective_mappings` | `compliance/<crate>.json` objective statuses + raw standards-pack identity |
//! | `reviews_approvals` | always [`DiffCategoryStatus::Unverifiable`] — see below |
//! | `anomalies` | `index.tool_command_failures` rows |
//! | `tool_identity` | engine version/sha/source + `env.json` toolchain fields + standards-pack identity |
//! | `integrity` | `BUNDLE.sig` + `SHA256SUMS` presence |
//! | `completeness_states` | `index.completeness` per-area states |
//! | `content_hash` | `index.content_hash` whole-content backstop |
//!
//! # Status semantics
//!
//! - [`DiffCategoryStatus::Equal`] — the category compared and is
//!   identical.
//! - [`DiffCategoryStatus::Added`] / [`DiffCategoryStatus::Removed`]
//!   — the category's evidence exists only in bundle B / only in
//!   bundle A (legitimate capture variance, e.g. a `--skip-tests`
//!   bundle diffed against a full-suite bundle).
//! - [`DiffCategoryStatus::Changed`] — comparable content differs.
//! - [`DiffCategoryStatus::Unverifiable`] — the category's artifacts
//!   are missing or unparseable where content is expected, so no
//!   equality claim is possible. Unverifiable is never silently
//!   downgraded to Equal: a report must not imply unexamined content
//!   is unchanged. When a category yields both real differences and
//!   unexaminable parts, the status is Changed and the unexaminable
//!   parts appear as `!`-prefixed detail lines.
//!
//! `reviews_approvals` is always Unverifiable: review and approval
//! records are workspace-corpus state, not bundle artifacts, so no
//! bundle pair can be compared on them from bundle content alone.
//!
//! # Deliberate exclusions
//!
//! Compared for equality are pass/fail and presence only. Per-test
//! `duration_ms` (host timing noise) and `failure_message` text
//! (diagnostic prose, not identity) are excluded from row equality;
//! command-row `cwd` (host-specific) is excluded likewise.
//!
//! # No diagnostic codes
//!
//! [`DiffError`] is an uncoded thiserror family (the
//! [`crate::corpus::CorpusError`] precedent): the comparison is a
//! report, not a diagnostic stream. Only genuine operational
//! failures are errors — missing category artifacts are statuses,
//! never errors.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::bundle::EvidenceIndex;

mod categories_assurance;
mod categories_capture;
mod categories_content;

/// Per-category comparison verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffCategoryStatus {
    /// The category compared and is identical on both sides.
    Equal,
    /// The category's evidence exists only in bundle B.
    Added,
    /// The category's evidence exists only in bundle A.
    Removed,
    /// Comparable content differs between the two sides.
    Changed,
    /// The category could not be compared (missing or unparseable
    /// artifacts, or content that is not bundle-carried at all).
    /// Never implies equality.
    Unverifiable,
}

/// One category's comparison result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CategoryDiff {
    /// Stable snake_case category name (see [`CATEGORY_ORDER`]).
    pub category: &'static str,
    /// The category verdict.
    pub status: DiffCategoryStatus,
    /// Deterministic, sorted, human-readable change lines. `+ `
    /// marks content present only in bundle B, `- ` only in bundle
    /// A, `~ ` changed content, `! ` a part that could not be
    /// examined. Empty when the category is Equal with nothing to
    /// note.
    pub details: Vec<String>,
}

/// The category names in fixed report order — the order
/// [`compare_bundles`] returns them. Documented and stable so
/// consumers can rely on report layout.
pub const CATEGORY_ORDER: &[&str] = &[
    "scope",
    "trace_graph",
    "tests",
    "coverage",
    "commands",
    "recipe",
    "inputs",
    "outputs",
    "objective_mappings",
    "reviews_approvals",
    "anomalies",
    "tool_identity",
    "integrity",
    "completeness_states",
    "content_hash",
];

/// Errors from [`compare_bundles`]. Only genuine operational
/// failures are errors — a missing category artifact yields an
/// [`DiffCategoryStatus::Unverifiable`] status, not an error.
///
/// Deliberately uncoded (no [`crate::diagnostic::DiagnosticCode`]
/// impl), same as [`crate::corpus::CorpusError`]: the comparison is
/// a library API, not a diagnostic surface.
#[derive(Debug, Error)]
pub enum DiffError {
    /// One of the two bundle roots is not a directory.
    #[error("bundle directory not found: {0:?}")]
    BundleNotFound(PathBuf),
    /// A file that exists could not be read.
    #[error("reading {path:?}")]
    Io {
        /// File whose read failed.
        path: PathBuf,
        /// Underlying OS error.
        #[source]
        source: std::io::Error,
    },
}

/// Compare two bundle directories across every assurance-relevant
/// category. Returns one [`CategoryDiff`] per category in
/// [`CATEGORY_ORDER`] order, with details sorted inside each
/// category.
///
/// # Errors
///
/// Returns [`DiffError::BundleNotFound`] when either root is not a
/// directory, and [`DiffError::Io`] when a file that exists cannot
/// be read. Missing or unparseable artifacts produce Unverifiable
/// statuses, not errors.
pub fn compare_bundles(a: &Path, b: &Path) -> Result<Vec<CategoryDiff>, DiffError> {
    if !a.is_dir() {
        return Err(DiffError::BundleNotFound(a.to_path_buf()));
    }
    if !b.is_dir() {
        return Err(DiffError::BundleNotFound(b.to_path_buf()));
    }
    let side_a = Side::load(a)?;
    let side_b = Side::load(b)?;
    let mut out = vec![
        categories_content::scope(&side_a, &side_b),
        categories_content::trace_graph(&side_a, &side_b)?,
        categories_capture::tests(&side_a, &side_b)?,
        categories_capture::coverage(&side_a, &side_b)?,
        categories_capture::commands(&side_a, &side_b)?,
        categories_content::recipe(&side_a, &side_b)?,
        categories_content::digest_plane(&side_a, &side_b, "inputs")?,
        categories_content::digest_plane(&side_a, &side_b, "outputs")?,
        categories_assurance::objective_mappings(&side_a, &side_b)?,
        categories_assurance::reviews_approvals(),
        categories_assurance::anomalies(&side_a, &side_b),
        categories_assurance::tool_identity(&side_a, &side_b)?,
        categories_assurance::integrity(&side_a, &side_b),
        categories_assurance::completeness_states(&side_a, &side_b),
        categories_assurance::content_hash(&side_a, &side_b),
    ];
    for diff in &mut out {
        finish(diff);
    }
    debug_assert_eq!(
        out.iter().map(|d| d.category).collect::<Vec<_>>(),
        CATEGORY_ORDER,
        "category list must match CATEGORY_ORDER exactly"
    );
    Ok(out)
}

/// Sort and dedup a category's detail lines — the determinism
/// contract. Called once per category by [`compare_bundles`].
pub(crate) fn finish(diff: &mut CategoryDiff) {
    diff.details.sort();
    diff.details.dedup();
}

/// Build an Unverifiable category result with a single `!`-prefixed
/// reason line.
pub(crate) fn unverifiable(category: &'static str, reason: impl Into<String>) -> CategoryDiff {
    CategoryDiff {
        category,
        status: DiffCategoryStatus::Unverifiable,
        details: vec![format!("! {}", reason.into())],
    }
}

/// A file that loaded cleanly, is absent, or is present but not
/// parseable. Absent-vs-unparseable matters for honest reasons.
pub(crate) enum Load<T> {
    /// File present and parsed.
    Ok(T),
    /// File absent.
    Missing,
    /// File present but not parseable into the expected shape.
    Unparseable,
}

impl<T> Load<T> {
    /// Human label of the non-Ok states, for reason strings.
    pub(crate) fn state_label(&self) -> &'static str {
        match self {
            Load::Ok(_) => "present",
            Load::Missing => "missing",
            Load::Unparseable => "unparseable",
        }
    }
}

/// One side of the comparison: the bundle root plus its parsed
/// index (the artifact most categories read).
pub(crate) struct Side {
    pub(crate) root: PathBuf,
    pub(crate) index: Load<EvidenceIndex>,
}

impl Side {
    fn load(root: &Path) -> Result<Self, DiffError> {
        let index = read_json_file::<EvidenceIndex>(root, "index.json")?;
        Ok(Side {
            root: root.to_path_buf(),
            index,
        })
    }
}

/// Read + parse one bundle JSON artifact. Absent → [`Load::Missing`];
/// present-but-unparseable → [`Load::Unparseable`]; a genuine read
/// error is the only `Err` path.
pub(crate) fn read_json_file<T: serde::de::DeserializeOwned>(
    root: &Path,
    rel: &str,
) -> Result<Load<T>, DiffError> {
    let path = root.join(rel);
    if !path.exists() {
        return Ok(Load::Missing);
    }
    let bytes = std::fs::read(&path).map_err(|source| DiffError::Io {
        path: path.clone(),
        source,
    })?;
    Ok(match serde_json::from_slice::<T>(&bytes) {
        Ok(value) => Load::Ok(value),
        Err(_) => Load::Unparseable,
    })
}

/// Whether a bundle-relative file exists on this side.
pub(crate) fn file_exists(side: &Side, rel: &str) -> bool {
    side.root.join(rel).is_file()
}

/// Extract both indexes for an index-dependent category, or the
/// Unverifiable result naming the side that blocks comparison.
pub(crate) fn require_indexes<'a>(
    a: &'a Side,
    b: &'a Side,
    category: &'static str,
) -> Result<(&'a EvidenceIndex, &'a EvidenceIndex), CategoryDiff> {
    match (&a.index, &b.index) {
        (Load::Ok(ia), Load::Ok(ib)) => Ok((ia, ib)),
        (Load::Ok(_), bad) => Err(unverifiable(
            category,
            format!("index.json {} in bundle B", bad.state_label()),
        )),
        (bad, Load::Ok(_)) => Err(unverifiable(
            category,
            format!("index.json {} in bundle A", bad.state_label()),
        )),
        (bad_a, bad_b) => Err(unverifiable(
            category,
            format!(
                "index.json {} in bundle A and {} in bundle B",
                bad_a.state_label(),
                bad_b.state_label()
            ),
        )),
    }
}

/// Push a `~ field: a -> b` detail when two string-ish values differ.
pub(crate) fn push_field_change(
    details: &mut Vec<String>,
    label: &str,
    a: &str,
    b: &str,
    changed: &mut bool,
) {
    if a != b {
        *changed = true;
        details.push(format!("~ {label}: {a} -> {b}"));
    }
}

// Tests live in a sibling file pulled in via `#[path]` so this
// module stays under the workspace 500-line limit.
#[cfg(test)]
#[path = "diff/tests.rs"]
mod tests;
