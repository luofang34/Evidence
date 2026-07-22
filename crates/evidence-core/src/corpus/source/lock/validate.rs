//! Strict lock parsing, the three ordered committed-lock
//! validation gates, and the blocking lock reader (LLR-134,
//! LLR-135).
//!
//! [`parse_lock`] parses committed bytes under the strict
//! versioned wire schema: unknown fields, malformed TOML, unknown
//! availability or capture-mode tags, malformed digests, a digest
//! or capture field on an unavailable entry, and an
//! external-control table inconsistent with the capture mode all
//! fail closed, as does a `schema_version` newer than
//! [`SUPPORTED_LOCK_SCHEMA`] and a document key carried by more
//! than one entry. Entry order is accepted as written —
//! canonicality is gate two's concern.
//!
//! [`validate_committed_lock`] validates the graph first, then
//! applies the three ordered gates:
//!
//! 1. Parse the committed bytes under the strict schema
//!    ([`SourceLockError::Parse`], [`SourceLockError::SchemaTooNew`],
//!    [`SourceLockError::DuplicateKey`]).
//! 2. Require the original bytes to equal the canonical rendering
//!    of the parsed value ([`SourceLockError::NonCanonical`]) — so
//!    non-canonical entry order, field order, whitespace, quoting,
//!    comments, or trailing-newline form fail even when the parsed
//!    values are equivalent.
//! 3. Require the parsed inventory to equal the projection derived
//!    from the validated graph ([`SourceLockError::Missing`],
//!    [`SourceLockError::Extra`], [`SourceLockError::Changed`]).
//!
//! [`validate_lock_file_blocking`] is the file-level validation
//! entry: it reads the committed file bytes once and applies the
//! full three gates to them, so the canonicality gate sees the exact
//! committed bytes. [`read_lock_blocking`] is the value-only
//! blocking reader returning the parsed lock; the parsed value alone
//! cannot support a canonical-byte check. Neither writes; validation
//! never mutates the workspace.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::Deserialize;

use super::super::super::digest::SourceContentDigest;
use super::super::super::graph::CorpusGraph;
use super::super::error::SourceError;
use super::error::SourceLockError;
use super::{
    ExternalControlId, LockCapture, LockMaterial, SUPPORTED_LOCK_SCHEMA, SourceLock,
    SourceLockEntry, derive_lock, render_lock_canonical,
};

/// On-disk shape of a `sources.lock` file. Strict: unknown fields
/// are a parse error.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LockFileWire {
    /// Lock schema version; newer than supported refuses to load.
    schema_version: u32,
    /// The per-document-key entries, in committed file order.
    #[serde(default)]
    entries: Vec<EntryWire>,
}

/// The `capture_mode` wire tag: a flat string field of the
/// committed file, mirrored here for deserialization only. The
/// domain type is [`LockCapture`]; because the external-control
/// identity travels as the sibling `external_control` field, the
/// wire cannot deserialize into [`LockCapture`] directly —
/// `entry_from_wire` pairs the tag with the identity and fails
/// closed on an inconsistent combination.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum LockCaptureMode {
    /// `vendored`.
    Vendored,
    /// `hash_only`.
    HashOnly,
    /// `external_controlled`.
    ExternalControlled,
}

/// One lock entry's wire shape. Internally tagged on
/// `availability` with unknown fields denied: an `available` entry
/// carries `sha256`, `capture_mode`, and optionally
/// `external_control`; an `unavailable` entry carries neither — a
/// digest on an unavailable entry fails deserialization rather
/// than being invented or silently dropped.
#[derive(Debug, Deserialize)]
#[serde(tag = "availability", rename_all = "snake_case", deny_unknown_fields)]
enum EntryWire {
    /// An available effective head.
    Available {
        /// Stable lineage key of the logical document.
        document_key: String,
        /// Uid of the effective source revision.
        source_uid: String,
        /// Declared lowercase content SHA-256.
        sha256: SourceContentDigest,
        /// Capture mode.
        capture_mode: LockCaptureMode,
        /// Immutable external control identity; required iff the
        /// capture mode is external-controlled, forbidden otherwise
        /// (checked after deserialization).
        external_control: Option<ExternalControlId>,
    },
    /// An unavailable effective head: no digest, no capture mode.
    Unavailable {
        /// Stable lineage key of the logical document.
        document_key: String,
        /// Uid of the effective source revision.
        source_uid: String,
    },
}

/// Parse strict `sources.lock` bytes into a [`SourceLock`]
/// (LLR-134). Entries keep the committed file order — canonical
/// order is the canonicality gate's concern, not the parser's.
///
/// # Errors
///
/// - [`SourceLockError::Parse`] on malformed TOML, non-UTF-8
///   bytes, an unknown field, an unknown availability or
///   capture-mode tag, a malformed digest, a digest or capture
///   field on an unavailable entry, or an external-control table
///   inconsistent with the capture mode.
/// - [`SourceLockError::SchemaTooNew`] on a `schema_version` newer
///   than [`SUPPORTED_LOCK_SCHEMA`].
/// - [`SourceLockError::DuplicateKey`] on a document key carried by
///   more than one entry.
pub fn parse_lock(bytes: &[u8]) -> Result<SourceLock, SourceError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| parse_error("sources.lock bytes are not valid UTF-8"))?;
    let wire: LockFileWire =
        toml::from_str(text).map_err(|source| SourceLockError::Parse { source })?;
    if wire.schema_version > SUPPORTED_LOCK_SCHEMA {
        return Err(SourceLockError::SchemaTooNew {
            found: wire.schema_version,
            supported: SUPPORTED_LOCK_SCHEMA,
        }
        .into());
    }
    let mut entries = Vec::with_capacity(wire.entries.len());
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for entry in &wire.entries {
        let document_key = match entry {
            EntryWire::Available { document_key, .. } => document_key,
            EntryWire::Unavailable { document_key, .. } => document_key,
        };
        if !seen.insert(document_key.as_str()) {
            return Err(SourceLockError::DuplicateKey {
                document_key: document_key.clone(),
            }
            .into());
        }
        entries.push(entry_from_wire(entry)?);
    }
    Ok(SourceLock {
        schema_version: wire.schema_version,
        entries,
    })
}

/// Validate committed `sources.lock` bytes against `graph` through
/// the three ordered gates pinned by the module docs (LLR-135).
/// `graph` is validated first — a failure surfaces as
/// [`SourceLockError::InvalidGraph`] carrying the
/// [`CorpusError`](super::super::super::error::CorpusError) — so an
/// `Ok` result means the committed lock is the canonical rendering
/// of a validated graph's effective heads. Pure apart from the
/// graph's own in-memory validation: no I/O, no workspace mutation.
///
/// # Errors
///
/// - [`SourceLockError::InvalidGraph`] when `graph` fails
///   [`CorpusGraph::validate`].
/// - [`SourceLockError::Parse`], [`SourceLockError::SchemaTooNew`],
///   [`SourceLockError::DuplicateKey`] at gate 1.
/// - [`SourceLockError::NonCanonical`] at gate 2.
/// - [`SourceLockError::Missing`], [`SourceLockError::Extra`],
///   [`SourceLockError::Changed`] at gate 3.
pub fn validate_committed_lock(bytes: &[u8], graph: &CorpusGraph) -> Result<(), SourceError> {
    graph
        .validate()
        .map_err(|source| SourceLockError::InvalidGraph {
            source: Box::new(source),
        })?;
    let committed = parse_lock(bytes)?;
    let canonical = render_lock_canonical(&committed);
    if canonical.as_slice() != bytes {
        return Err(SourceLockError::NonCanonical {
            detail: first_difference(bytes, &canonical),
        }
        .into());
    }
    let derived = derive_lock(graph);
    diff_inventories(&derived, &committed)?;
    Ok(())
}

/// Read and parse the committed lock at `path` (LLR-135). This is
/// the value-only blocking reader: it returns the parsed lock and
/// never writes. The parsed value alone cannot support a
/// canonical-byte check — for file-level validation against a
/// graph, use [`validate_lock_file_blocking`], which keeps the
/// committed bytes for the canonicality gate. Parse failures
/// surface exactly as [`parse_lock`] reports them.
///
/// # Errors
///
/// - [`SourceLockError::Read`] when the file cannot be read.
/// - Every [`parse_lock`] failure when the bytes are degenerate.
pub fn read_lock_blocking(path: &Path) -> Result<SourceLock, SourceError> {
    let bytes = std::fs::read(path).map_err(|source| SourceLockError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    parse_lock(&bytes)
}

/// Validate the committed lock file at `path` against `graph`
/// through the full three ordered gates of
/// [`validate_committed_lock`] (LLR-135). This is the file-level
/// validation entry: it reads the file bytes once and keeps them,
/// so the canonicality gate applies to the exact committed bytes —
/// a parsed value alone cannot support that check. It never
/// writes; validation never mutates the workspace.
///
/// # Errors
///
/// - [`SourceLockError::Read`] when the file cannot be read.
/// - Every [`validate_committed_lock`] failure when the bytes are
///   degenerate or disagree with `graph`.
pub fn validate_lock_file_blocking(path: &Path, graph: &CorpusGraph) -> Result<(), SourceError> {
    let bytes = std::fs::read(path).map_err(|source| SourceLockError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    validate_committed_lock(&bytes, graph)
}

/// Compare the derived inventory with the committed one in sorted
/// document-key order, reporting the first difference: a missing
/// entry, an extra entry, then a changed field. Duplicate
/// committed keys were rejected at parse, so each key names at
/// most one committed entry.
fn diff_inventories(derived: &SourceLock, committed: &SourceLock) -> Result<(), SourceLockError> {
    let derived_by_key = by_document_key(derived);
    let committed_by_key = by_document_key(committed);
    for document_key in derived_by_key.keys() {
        if !committed_by_key.contains_key(document_key) {
            return Err(SourceLockError::Missing {
                document_key: (*document_key).to_string(),
            });
        }
    }
    for document_key in committed_by_key.keys() {
        if !derived_by_key.contains_key(document_key) {
            return Err(SourceLockError::Extra {
                document_key: (*document_key).to_string(),
            });
        }
    }
    for (document_key, derived_entry) in &derived_by_key {
        let committed_entry = committed_by_key[document_key];
        if let Some(field) = first_changed_field(derived_entry, committed_entry) {
            return Err(SourceLockError::Changed {
                document_key: (*document_key).to_string(),
                field,
            });
        }
    }
    Ok(())
}

/// Index a lock's entries by document key. Keys are unique on
/// validated inputs (parse rejects duplicates; derivation binds one
/// head per key).
fn by_document_key(lock: &SourceLock) -> BTreeMap<&str, &SourceLockEntry> {
    lock.entries
        .iter()
        .map(|entry| (entry.document_key.as_str(), entry))
        .collect()
}

/// The first differing bound field between two entries of one
/// document key, in the canonical field order: uid, availability,
/// digest, capture mode, external identity.
fn first_changed_field(
    derived: &SourceLockEntry,
    committed: &SourceLockEntry,
) -> Option<&'static str> {
    if committed.source_uid != derived.source_uid {
        return Some("uid");
    }
    match (&derived.material, &committed.material) {
        (LockMaterial::Unavailable, LockMaterial::Unavailable) => None,
        (LockMaterial::Available { .. }, LockMaterial::Unavailable)
        | (LockMaterial::Unavailable, LockMaterial::Available { .. }) => Some("availability"),
        (
            LockMaterial::Available {
                sha256: derived_sha256,
                capture: derived_capture,
            },
            LockMaterial::Available {
                sha256: committed_sha256,
                capture: committed_capture,
            },
        ) => {
            if committed_sha256 != derived_sha256 {
                return Some("digest");
            }
            if committed_capture.as_str() != derived_capture.as_str() {
                return Some("capture_mode");
            }
            if external_control(committed_capture) != external_control(derived_capture) {
                return Some("external_identity");
            }
            None
        }
    }
}

/// The external control identity a capture mode binds, if any.
fn external_control(capture: &LockCapture) -> Option<&ExternalControlId> {
    match capture {
        LockCapture::ExternalControlled(external) => Some(external),
        _ => None,
    }
}

/// Where committed bytes first differ from the canonical rendering:
/// the first differing byte offset, or the length mismatch when one
/// form is a prefix of the other.
fn first_difference(committed: &[u8], canonical: &[u8]) -> String {
    for (index, (left, right)) in committed.iter().zip(canonical.iter()).enumerate() {
        if left != right {
            return format!(
                "first difference at byte {index}: committed byte 0x{left:02X}, \
                 canonical byte 0x{right:02X}"
            );
        }
    }
    format!(
        "length differs: committed is {} bytes, the canonical rendering is {} bytes",
        committed.len(),
        canonical.len()
    )
}

/// Convert one wire entry into the lock value, enforcing the
/// capture-mode/external-control consistency the flat wire fields
/// imply: the external-control table is present iff the capture
/// mode is external-controlled. The resulting [`LockMaterial`]
/// carries the consistent combination by construction.
fn entry_from_wire(wire: &EntryWire) -> Result<SourceLockEntry, SourceError> {
    match wire {
        EntryWire::Available {
            document_key,
            source_uid,
            sha256,
            capture_mode,
            external_control,
        } => {
            let capture = match (capture_mode, external_control) {
                (LockCaptureMode::Vendored, None) => LockCapture::Vendored,
                (LockCaptureMode::HashOnly, None) => LockCapture::HashOnly,
                (LockCaptureMode::ExternalControlled, Some(external)) => {
                    LockCapture::ExternalControlled(external.clone())
                }
                _ => {
                    return Err(parse_error(
                        "external_control is required iff capture_mode is \"external_controlled\"",
                    ));
                }
            };
            Ok(SourceLockEntry {
                document_key: document_key.clone(),
                source_uid: source_uid.clone(),
                material: LockMaterial::Available {
                    sha256: sha256.clone(),
                    capture,
                },
            })
        }
        EntryWire::Unavailable {
            document_key,
            source_uid,
        } => Ok(SourceLockEntry {
            document_key: document_key.clone(),
            source_uid: source_uid.clone(),
            material: LockMaterial::Unavailable,
        }),
    }
}

/// A gate-1 parse failure with a message this layer authors (the
/// TOML parser cannot produce: non-UTF-8 bytes and the
/// capture-mode/external-control invariant).
fn parse_error(message: &str) -> SourceError {
    SourceLockError::Parse {
        source: <toml::de::Error as serde::de::Error>::custom(message),
    }
    .into()
}
