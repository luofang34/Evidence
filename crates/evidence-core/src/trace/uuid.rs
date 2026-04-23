//! UUID assignment + on-disk backfill for trace entries.
//!
//! Every entry kind (HLR / LLR / Test / Derived) declares `uid: Option<String>`
//! with `#[serde(default)]` so legacy TOML without `uid` fields parses.
//! `assign_valid_uuids_*` fills in a fresh v4 UUID for every entry
//! whose `uid` is missing OR not a valid UUID (e.g. init-template
//! placeholders like `"HLR-001"`); `backfill_uuids` reads each
//! trace file, rewrites both `uid` fields and cross-file
//! `traces_to` references so placeholder IDs stay consistent
//! after the rewrite, and writes the file back with
//! `toml::to_string_pretty` only if any change was made.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;

use super::entries::{
    DerivedEntry, DerivedFile, HlrEntry, HlrFile, LlrEntry, LlrFile, TestEntry, TestsFile,
};
use super::read::{TraceReadError, read_toml};
use crate::diagnostic::{DiagnosticCode, Location, Severity};

/// `true` if `uid` is missing or not a valid UUID. The backfill
/// path treats these identically — the uid slot needs a new v4.
/// Init-template placeholders like `"HLR-001"` satisfy this
/// predicate and are replaced on first backfill.
fn needs_new_uuid(uid: Option<&str>) -> bool {
    uid.is_none_or(|s| uuid::Uuid::parse_str(s).is_err())
}

/// If `uid` is missing or not a valid UUID, replace it with a
/// fresh v4. Returns `Some(old)` when the slot held a non-None
/// non-UUID string (caller needs to rewrite `traces_to`
/// references to the old value); returns `None` when the slot
/// was either already valid (no change) or was `None` (replaced,
/// but nothing could reference a `None` old value).
///
/// Used by each `assign_valid_uuids_*` helper — keeps the logic
/// single-sourced across the four entry kinds.
fn rewrite_uid_if_needed(uid: &mut Option<String>) -> Option<(Option<String>, String)> {
    if !needs_new_uuid(uid.as_deref()) {
        return None;
    }
    let old = uid.take();
    let new = uuid::Uuid::new_v4().to_string();
    *uid = Some(new.clone());
    Some((old, new))
}

fn record_rewrite(
    outcome: Option<(Option<String>, String)>,
    remap: &mut BTreeMap<String, String>,
) -> bool {
    match outcome {
        Some((Some(old), new)) => {
            remap.insert(old, new);
            true
        }
        Some((None, _)) => true,
        None => false,
    }
}

/// Errors returned by [`backfill_uuids`].
#[derive(Debug, Error)]
pub enum BackfillError {
    /// Reading / parsing one of the trace TOML files failed.
    #[error(transparent)]
    Read(#[from] TraceReadError),
    /// Writing the updated TOML file back to disk failed.
    #[error("writing {path}")]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// `toml::to_string_pretty` round-trip serialization failed.
    /// Boxed to keep the enum under clippy's `result_large_err`
    /// threshold on Windows.
    #[error("serializing {filename}")]
    Serialize {
        filename: &'static str,
        #[source]
        source: Box<toml::ser::Error>,
    },
}

impl DiagnosticCode for BackfillError {
    fn code(&self) -> &'static str {
        // `Read(_)` wraps TraceReadError but keeps its own BACKFILL_*
        // code; the caller surfacing a backfill failure is a different
        // operational signal than a plain trace-read — distinguishing
        // them lets an agent route the diagnostic to the right
        // remediation.
        match self {
            BackfillError::Read(_) => "TRACE_BACKFILL_READ_FAILED",
            BackfillError::Write { .. } => "TRACE_BACKFILL_WRITE_FAILED",
            BackfillError::Serialize { .. } => "TRACE_BACKFILL_SERIALIZE_FAILED",
        }
    }

    fn severity(&self) -> Severity {
        Severity::Error
    }

    fn location(&self) -> Option<Location> {
        match self {
            BackfillError::Write { path, .. } => Some(Location {
                file: Some(path.clone()),
                ..Location::default()
            }),
            BackfillError::Serialize { filename, .. } => Some(Location {
                file: Some(PathBuf::from(filename)),
                ..Location::default()
            }),
            BackfillError::Read(inner) => inner.location(),
        }
    }
}

/// Assign fresh v4 UUIDs to HLR entries whose `uid` is missing
/// or is not a valid UUID (e.g. init-template placeholders like
/// `"HLR-001"`). Returns `(count, remap)` where `remap` keys are
/// the non-None pre-rewrite values and values are the new UUIDs,
/// so callers can rewrite `traces_to` references in the same
/// file or in downstream files (LLR → HLR, Test → LLR).
pub fn assign_valid_uuids_hlr(entries: &mut [HlrEntry]) -> (usize, BTreeMap<String, String>) {
    let mut count = 0;
    let mut remap = BTreeMap::new();
    for entry in entries.iter_mut() {
        if record_rewrite(rewrite_uid_if_needed(&mut entry.uid), &mut remap) {
            count += 1;
        }
    }
    (count, remap)
}

/// Assign fresh v4 UUIDs to LLR entries with missing or invalid
/// `uid`. Same semantics as [`assign_valid_uuids_hlr`].
pub fn assign_valid_uuids_llr(entries: &mut [LlrEntry]) -> (usize, BTreeMap<String, String>) {
    let mut count = 0;
    let mut remap = BTreeMap::new();
    for entry in entries.iter_mut() {
        if record_rewrite(rewrite_uid_if_needed(&mut entry.uid), &mut remap) {
            count += 1;
        }
    }
    (count, remap)
}

/// Assign fresh v4 UUIDs to Test entries with missing or invalid
/// `uid`. Same semantics as [`assign_valid_uuids_hlr`].
pub fn assign_valid_uuids_test(entries: &mut [TestEntry]) -> (usize, BTreeMap<String, String>) {
    let mut count = 0;
    let mut remap = BTreeMap::new();
    for entry in entries.iter_mut() {
        if record_rewrite(rewrite_uid_if_needed(&mut entry.uid), &mut remap) {
            count += 1;
        }
    }
    (count, remap)
}

/// Assign fresh v4 UUIDs to Derived entries with missing or
/// invalid `uid`. Derived entries have no `traces_to` field so
/// the returned remap is only useful if another layer references
/// derived UIDs (not the current schema, but kept symmetric so
/// future cross-references work).
pub fn assign_valid_uuids_derived(
    entries: &mut [DerivedEntry],
) -> (usize, BTreeMap<String, String>) {
    let mut count = 0;
    let mut remap = BTreeMap::new();
    for entry in entries.iter_mut() {
        if record_rewrite(rewrite_uid_if_needed(&mut entry.uid), &mut remap) {
            count += 1;
        }
    }
    (count, remap)
}

/// Apply a uid remap to a `traces_to` vector in place. Strings
/// not present in the remap are left unchanged — they're either
/// already valid UUIDs or references to entries outside the
/// current backfill scope.
fn rewrite_traces_to(refs: &mut [String], remap: &BTreeMap<String, String>) -> bool {
    let mut changed = false;
    for r in refs.iter_mut() {
        if let Some(new) = remap.get(r) {
            *r = new.clone();
            changed = true;
        }
    }
    changed
}

/// Merge a per-layer remap into the cross-layer union. Panics
/// when two layers coincidentally used the same placeholder key
/// (e.g. both `"x-001"` in hand-imported data) — silently
/// overwriting one layer's rewrite would corrupt that layer's
/// `traces_to` references, which is worse than aborting the
/// backfill. Fires in both debug and release so the invariant
/// holds on Nix (release-mode tests) and cert-profile runs.
fn merge_remap(dst: &mut BTreeMap<String, String>, src: BTreeMap<String, String>) {
    for (old, new) in src {
        assert!(
            !dst.contains_key(&old),
            "duplicate placeholder uid across trace layers: '{}' \
             (existing: '{}', new: '{}'); two layers' entries share \
             the same placeholder — fix by using distinct placeholders \
             per layer or migrate hand-authored uids to real UUIDs \
             before backfill",
            old,
            dst[&old],
            new
        );
        dst.insert(old, new);
    }
}

/// Read trace files from a directory, assign fresh v4 UUIDs to
/// every entry whose `uid` is missing or not a valid UUID, and
/// write the files back. Rewrites cross-file `traces_to`
/// references so init-template placeholders stay consistent
/// after the first backfill (e.g. an LLR that traced to the
/// placeholder `"HLR-001"` picks up the same UUID the HLR-001
/// entry just got assigned).
///
/// Returns total number of UUIDs assigned across all files.
/// Writes only the files that actually changed, so repeated
/// backfills on a fully-valid trace tree are free.
pub fn backfill_uuids(trace_root: &str) -> Result<usize, BackfillError> {
    let root_path = Path::new(trace_root);
    let mut total = 0;

    // Phase 1: load every file that exists.
    let sys_path = root_path.join("sys.toml");
    let hlr_path = root_path.join("hlr.toml");
    let llr_path = root_path.join("llr.toml");
    let tests_path = root_path.join("tests.toml");
    let derived_path = root_path.join("derived.toml");

    let mut sys: Option<HlrFile> = if sys_path.exists() {
        Some(read_toml(&sys_path)?)
    } else {
        None
    };
    let mut hlr: Option<HlrFile> = if hlr_path.exists() {
        Some(read_toml(&hlr_path)?)
    } else {
        None
    };
    let mut llr: Option<LlrFile> = if llr_path.exists() {
        Some(read_toml(&llr_path)?)
    } else {
        None
    };
    let mut tests: Option<TestsFile> = if tests_path.exists() {
        Some(read_toml(&tests_path)?)
    } else {
        None
    };
    let mut derived: Option<DerivedFile> = if derived_path.exists() {
        Some(read_toml(&derived_path)?)
    } else {
        None
    };

    // Phase 2: rewrite uids + collect a unified remap across
    // every file. Key: pre-rewrite uid string (e.g. "HLR-001");
    // value: new v4 UUID.
    let mut remap: BTreeMap<String, String> = BTreeMap::new();
    let mut sys_changed = false;
    let mut hlr_changed = false;
    let mut llr_changed = false;
    let mut tests_changed = false;
    let mut derived_changed = false;

    if let Some(s) = sys.as_mut() {
        let (n, m) = assign_valid_uuids_hlr(&mut s.requirements);
        total += n;
        if n > 0 {
            sys_changed = true;
        }
        merge_remap(&mut remap, m);
    }
    if let Some(h) = hlr.as_mut() {
        let (n, m) = assign_valid_uuids_hlr(&mut h.requirements);
        total += n;
        if n > 0 {
            hlr_changed = true;
        }
        merge_remap(&mut remap, m);
    }
    if let Some(l) = llr.as_mut() {
        let (n, m) = assign_valid_uuids_llr(&mut l.requirements);
        total += n;
        if n > 0 {
            llr_changed = true;
        }
        merge_remap(&mut remap, m);
    }
    if let Some(t) = tests.as_mut() {
        let (n, m) = assign_valid_uuids_test(&mut t.tests);
        total += n;
        if n > 0 {
            tests_changed = true;
        }
        merge_remap(&mut remap, m);
    }
    if let Some(d) = derived.as_mut() {
        let (n, m) = assign_valid_uuids_derived(&mut d.requirements);
        total += n;
        if n > 0 {
            derived_changed = true;
        }
        merge_remap(&mut remap, m);
    }

    // Phase 3: apply the remap to every `traces_to` vector so
    // references to pre-rewrite placeholders pick up the new
    // UUIDs. Tracks per-file change flags independently from
    // uid rewrites — a file with no uid rewrites but
    // traces_to references into another file's rewritten uids
    // still needs to be written back.
    if let Some(h) = hlr.as_mut() {
        for entry in h.requirements.iter_mut() {
            if rewrite_traces_to(&mut entry.traces_to, &remap) {
                hlr_changed = true;
            }
        }
    }
    if let Some(l) = llr.as_mut() {
        for entry in l.requirements.iter_mut() {
            if rewrite_traces_to(&mut entry.traces_to, &remap) {
                llr_changed = true;
            }
        }
    }
    if let Some(t) = tests.as_mut() {
        for entry in t.tests.iter_mut() {
            if rewrite_traces_to(&mut entry.traces_to, &remap) {
                tests_changed = true;
            }
        }
    }

    // Phase 4: write back every file that changed.
    if sys_changed && let Some(s) = &sys {
        write_trace_file(&sys_path, s, "sys.toml")?;
    }
    if hlr_changed && let Some(h) = &hlr {
        write_trace_file(&hlr_path, h, "hlr.toml")?;
    }
    if llr_changed && let Some(l) = &llr {
        write_trace_file(&llr_path, l, "llr.toml")?;
    }
    if tests_changed && let Some(t) = &tests {
        write_trace_file(&tests_path, t, "tests.toml")?;
    }
    if derived_changed && let Some(d) = &derived {
        write_trace_file(&derived_path, d, "derived.toml")?;
    }

    Ok(total)
}

/// Serialize a trace file and write it to disk, mapping the
/// toml / I/O errors into the structured [`BackfillError`] variants.
fn write_trace_file<T: serde::Serialize>(
    path: &Path,
    value: &T,
    filename: &'static str,
) -> Result<(), BackfillError> {
    let content = toml::to_string_pretty(value).map_err(|source| BackfillError::Serialize {
        filename,
        source: Box::new(source),
    })?;
    fs::write(path, content).map_err(|source| BackfillError::Write {
        path: path.to_path_buf(),
        source,
    })
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
#[path = "uuid/tests.rs"]
mod tests;
