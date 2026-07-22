//! The canonical, derived `sources.lock` inventory of the active
//! frozen-source baseline (LLR-133, LLR-134, LLR-135).
//!
//! `cert/sources.lock` is the canonical inventory of the effective
//! source heads the corpus baseline selected. The source-revision
//! registry records remain authoritative; the lock has no
//! independent editable semantics and must equal a fresh projection
//! from the validated graph. [`derive_lock`] projects a
//! [`SourceLock`] from a graph's effective heads,
//! [`render_lock_canonical`] renders the one canonical byte form,
//! and the `validate` sibling parses strict lock bytes and compares
//! a committed lock with the derived projection. All four are pure;
//! the only I/O anywhere in the module is the `validate` sibling's
//! blocking pair —
//! [`validate_lock_file_blocking`](validate::validate_lock_file_blocking),
//! the file-level validation entry applying the three gates to the
//! committed file bytes, and
//! [`read_lock_blocking`](validate::read_lock_blocking), a
//! value-only blocking reader. Neither mutates the workspace.
//! Writing or replacing the committed lock belongs to a later human
//! CLI surface.
//!
//! # Entry membership
//!
//! One entry per effective `document_key`, sorted by document key
//! then source uid, binding: the document key; the effective source
//! revision uid; the availability state; the validated lowercase
//! content SHA-256 when the source is available; the capture mode
//! when available; and the immutable external control identity
//! (controlling system plus immutable identifier) when that
//! identity is part of the capture contract. Historical non-head
//! revisions remain in the registry and lineage but are never
//! active lock entries. An unavailable effective source is
//! represented explicitly — `availability = "unavailable"` with no
//! digest and no capture mode — and never receives a synthetic
//! digest.
//!
//! Excluded as non-identity: vendored file paths and record-file
//! layout (storage details, not source identity), retrieval
//! timestamps and unavailability reasons (audit metadata), titles,
//! media types, and canonical locations.
//!
//! # Canonical byte format (v1)
//!
//! [`render_lock_canonical`] emits exactly this TOML form; the
//! golden fixture `tests/fixtures/corpus/sources_lock_v1.golden`
//! byte-locks it:
//!
//! ```text
//! schema_version = 1
//!
//! [[entries]]
//! document_key = "DOC-1"
//! source_uid = "src_00000000-0000-4000-8000-0000000000a1"
//! availability = "available"
//! sha256 = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
//! capture_mode = "vendored"
//!
//! [[entries]]
//! document_key = "DOC-2"
//! source_uid = "src_00000000-0000-4000-8000-0000000000a2"
//! availability = "available"
//! sha256 = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
//! capture_mode = "external_controlled"
//! external_control = { system = "plm-hd", immutable_id = "DOC-2@revC" }
//!
//! [[entries]]
//! document_key = "DOC-3"
//! source_uid = "src_00000000-0000-4000-8000-0000000000a3"
//! availability = "unavailable"
//! ```
//!
//! The contract, precisely:
//!
//! 1. The first line is `schema_version = <N>` — the lock's schema
//!    version in decimal — followed by LF. Only
//!    [`SUPPORTED_LOCK_SCHEMA`] is emitted by [`derive_lock`].
//! 2. Entries are sorted by `document_key`, then `source_uid`
//!    ([`render_lock_canonical`] re-sorts, so no construction path
//!    affects the bytes). Each entry block is one blank line, the
//!    `[[entries]]` header, then the field lines; exactly one blank
//!    line separates the schema line from the first entry and each
//!    entry from the next.
//! 3. Field order is fixed: `document_key`, `source_uid`,
//!    `availability`; available entries continue `sha256`,
//!    `capture_mode`, and — only when the capture mode is
//!    `external_controlled` — `external_control`. Unavailable
//!    entries carry exactly the three leading fields.
//! 4. `availability` is `"available"` or `"unavailable"`;
//!    `capture_mode` is `"vendored"`, `"hash_only"`, or
//!    `"external_controlled"`. `sha256` is the digest's 64
//!    lowercase hexadecimal characters, quoted.
//! 5. `external_control` is an inline table in exactly this shape:
//!    `external_control = { system = "<system>", immutable_id = "<id>" }`
//!    — `{ ` and ` }` around the pairs, `, ` between them, ` = `
//!    around each equals sign.
//! 6. Every string is a TOML basic string with deterministic
//!    minimal escaping: `"` renders `\"`, `\` renders `\\`,
//!    U+0008/U+0009/U+000A/U+000C/U+000D render `\b`/`\t`/`\n`/`\f`/`\r`,
//!    every other C0 control and U+007F renders `\uXXXX` with
//!    uppercase hexadecimal, and all remaining characters —
//!    including non-ASCII — render as raw UTF-8.
//! 7. Line endings are LF everywhere. The file ends with the last
//!    field line plus a single LF — no trailing blank line, no
//!    comments. An empty lock is exactly `schema_version = <N>`
//!    plus one LF.
//!
//! Changing this contract requires a new lock schema version,
//! never a silent change of existing canonical bytes.
//!
//! # What the lock proves — and what it does not
//!
//! The lock proves which source digests and availability states the
//! corpus baseline selected. It does **not** prove recipe
//! reproducibility (how the bytes were produced), remote
//! retrievability (that `canonical_location` still serves them), or
//! vendored byte integrity (that payload files on disk still match
//! the declared digests — that is a verification pass over the
//! vendored tree, not a property of this inventory).
//!
//! Module map:
//!
//! - `error` — the [`SourceLockError`] taxonomy every lock failure
//!   reports through (LLR-135)
//! - `validate` — strict parsing, the three ordered committed-lock
//!   gates, and the blocking read and file-validation entries
//!   (LLR-134, LLR-135)

use super::super::digest::SourceContentDigest;
use super::super::graph::{CorpusGraph, Node, SourceCapture, SourceMaterial, SourceRevisionNode};
use super::lineage::effective_source_heads;

pub(crate) mod error;
mod validate;

pub use error::SourceLockError;
pub use validate::{
    parse_lock, read_lock_blocking, validate_committed_lock, validate_lock_file_blocking,
};

/// Highest sources-lock schema version this tool loads (LLR-134).
pub const SUPPORTED_LOCK_SCHEMA: u32 = 1;

/// The typed material state of one effective source head (LLR-133):
/// available material always carries its digest and capture mode;
/// unavailable material carries neither. The availability/digest/
/// capture consistency the flat canonical fields imply is a
/// type-level invariant here, not a prose one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LockMaterial {
    /// The revision's bytes were captured; the entry binds the
    /// declared digest and the capture mode.
    Available {
        /// Validated lowercase content SHA-256 — never a synthetic
        /// digest.
        sha256: SourceContentDigest,
        /// How the captured bytes are held.
        capture: LockCapture,
    },
    /// The revision's bytes could not be captured; the entry binds
    /// no digest and no capture mode.
    Unavailable,
}

/// The capture mode of an available effective source head
/// (LLR-133). Vendored payloads bind the mode alone — the vendored
/// path is a storage detail and never enters the lock. The
/// external-controlled mode always carries its immutable control
/// identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LockCapture {
    /// Raw bytes are vendored beneath the `sources/` payload root.
    Vendored,
    /// Only the digest and location are recorded.
    HashOnly,
    /// Bytes live in an external controlled document system under
    /// an immutable identifier.
    ExternalControlled(ExternalControlId),
}

impl LockCapture {
    /// The canonical `capture_mode` wire string for this mode.
    pub fn as_str(&self) -> &'static str {
        match self {
            LockCapture::Vendored => "vendored",
            LockCapture::HashOnly => "hash_only",
            LockCapture::ExternalControlled(_) => "external_controlled",
        }
    }
}

/// The immutable external control identity of an
/// external-controlled capture (LLR-133): the controlling system
/// and the immutable identifier within it. Strict wire shape —
/// unknown fields fail deserialization.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalControlId {
    /// The controlling system.
    pub system: String,
    /// The immutable identifier within that system.
    pub immutable_id: String,
}

/// One lock entry: the effective source head of one document key
/// (LLR-133).
///
/// The consistency the flat canonical fields imply — a digest and a
/// capture mode exactly when available, an external-control
/// identity exactly under the external-controlled mode — is a
/// type-level invariant of [`LockMaterial`] and [`LockCapture`],
/// not a prose one: an invalid combination is unrepresentable, so
/// there is no runtime test for unrepresentable states — the type
/// is the proof. Every construction path ([`derive_lock`],
/// [`parse_lock`], or a direct literal) yields a value
/// [`render_lock_canonical`] renders deterministically.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceLockEntry {
    /// Stable lineage key of the logical document.
    pub document_key: String,
    /// Uid of the effective source revision — the document's head.
    pub source_uid: String,
    /// Typed material state of the head.
    pub material: LockMaterial,
}

/// The derived source-inventory lock (LLR-133): a strict versioned
/// schema and one entry per effective document key.
///
/// Canonical form holds entries sorted by `document_key`, then
/// `source_uid`. [`derive_lock`] and [`render_lock_canonical`]
/// produce sorted order; [`parse_lock`] preserves the committed
/// file order and leaves order enforcement to the canonicality gate
/// of [`validate_committed_lock`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceLock {
    /// Lock schema version; [`SUPPORTED_LOCK_SCHEMA`] for every
    /// lock this tool derives.
    pub schema_version: u32,
    /// The per-document-key entries.
    pub entries: Vec<SourceLockEntry>,
}

/// Project the [`SourceLock`] of `graph` from its effective source
/// heads (LLR-133): one entry per document key, the head revision
/// no other revision supersedes. Historical non-head revisions
/// never appear. Available material binds its declared digest,
/// capture mode, and — for external-controlled capture — the
/// controlling system and immutable id; unavailable material binds
/// an explicit unavailable state with no digest and no capture
/// mode. Pure: no I/O, no environment reads; file layout, record
/// order, timestamps, and vendored paths never enter the result.
///
/// Read-only derived view: on a graph that passed
/// [`CorpusGraph::validate`] each document key yields exactly one
/// head. Validate first — [`validate_committed_lock`] does.
pub fn derive_lock(graph: &CorpusGraph) -> SourceLock {
    let mut entries = Vec::new();
    for (document_key, uid) in effective_source_heads(graph) {
        // `effective_source_heads` maps document keys to uids of
        // source-revision nodes it iterated, so the lookup always
        // succeeds; the `else` guards a future derivation change
        // without panicking.
        let Some(Node::SourceRevision(revision)) = graph.get(&uid) else {
            continue;
        };
        entries.push(entry_from_revision(document_key, uid, revision));
    }
    sort_entries(&mut entries);
    SourceLock {
        schema_version: SUPPORTED_LOCK_SCHEMA,
        entries,
    }
}

/// Project one effective head revision into its lock entry.
fn entry_from_revision(
    document_key: String,
    uid: String,
    revision: &SourceRevisionNode,
) -> SourceLockEntry {
    let material = match &revision.material {
        SourceMaterial::Available {
            sha256, capture, ..
        } => {
            let capture = match capture {
                SourceCapture::Vendored { .. } => LockCapture::Vendored,
                SourceCapture::HashOnly { .. } => LockCapture::HashOnly,
                SourceCapture::ExternalControlled {
                    system,
                    immutable_id,
                } => LockCapture::ExternalControlled(ExternalControlId {
                    system: system.clone(),
                    immutable_id: immutable_id.clone(),
                }),
            };
            LockMaterial::Available {
                sha256: sha256.clone(),
                capture,
            }
        }
        SourceMaterial::Unavailable { .. } => LockMaterial::Unavailable,
    };
    SourceLockEntry {
        document_key,
        source_uid: uid,
        material,
    }
}

/// The canonical entry order: `document_key`, then `source_uid`.
fn sort_entries(entries: &mut [SourceLockEntry]) {
    entries.sort_by(|a, b| (&a.document_key, &a.source_uid).cmp(&(&b.document_key, &b.source_uid)));
}

/// Render `lock` in the canonical v1 byte form pinned by the module
/// docs (LLR-134). Entries are re-sorted before rendering, so no
/// construction path affects the bytes. Pure and host-independent.
pub fn render_lock_canonical(lock: &SourceLock) -> Vec<u8> {
    let mut entries = lock.entries.clone();
    sort_entries(&mut entries);
    let mut out = String::new();
    out.push_str("schema_version = ");
    out.push_str(&lock.schema_version.to_string());
    out.push('\n');
    for entry in &entries {
        out.push_str("\n[[entries]]\n");
        push_field(&mut out, "document_key", &entry.document_key);
        push_field(&mut out, "source_uid", &entry.source_uid);
        match &entry.material {
            LockMaterial::Available { sha256, capture } => {
                push_field(&mut out, "availability", "available");
                push_field(&mut out, "sha256", sha256.as_str());
                push_field(&mut out, "capture_mode", capture.as_str());
                if let LockCapture::ExternalControlled(external) = capture {
                    out.push_str("external_control = { system = ");
                    push_basic_string(&mut out, &external.system);
                    out.push_str(", immutable_id = ");
                    push_basic_string(&mut out, &external.immutable_id);
                    out.push_str(" }\n");
                }
            }
            LockMaterial::Unavailable => {
                push_field(&mut out, "availability", "unavailable");
            }
        }
    }
    out.into_bytes()
}

/// One `key = "<value>"` line in canonical escaping.
fn push_field(out: &mut String, key: &str, value: &str) {
    out.push_str(key);
    out.push_str(" = ");
    push_basic_string(out, value);
    out.push('\n');
}

/// Append `value` as a TOML basic string with the deterministic
/// minimal escaping pinned by the module docs.
fn push_basic_string(out: &mut String, value: &str) {
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{08}' => out.push_str("\\b"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\u{0C}' => out.push_str("\\f"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x20 || (c as u32) == 0x7F => {
                out.push_str(&format!("\\u{:04X}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

// Tests live in sibling files pulled in via `#[path]`: shared
// fixtures plus one module per TEST entry.
#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
#[path = "lock/fixtures.rs"]
mod fixtures;
#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
#[path = "lock/tests.rs"]
mod tests;
#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
#[path = "lock/validate_tests.rs"]
mod validate_tests;
