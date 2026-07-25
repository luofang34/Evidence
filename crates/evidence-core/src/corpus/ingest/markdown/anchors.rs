//! Heading anchor extraction and the deterministic slugger
//! (LLR-162).
//!
//! A heading's anchor is its explicit `{#id}` suffix when one is
//! present and valid; otherwise a slug generated from the heading
//! text. Both feed the locator's `anchor` field and therefore the
//! structural key's first precedence tier.
//!
//! # Explicit ids
//!
//! [`extract_explicit_id`] inspects the trailing `{#...}` suffix of
//! the assembled heading text; when several brace groups trail, the
//! last one wins. A valid id is non-empty, starts with an ASCII
//! alphanumeric, and continues with ASCII alphanumerics, `-`, or
//! `_`. A trailing `{#...}` whose contents fail that rule is a
//! malformed explicit id: the caller emits a typed diagnostic and
//! treats the whole heading text as ordinary text.
//!
//! # Generated slugs
//!
//! [`slugify`] maps text to a slug: Unicode NFC, then lowercase,
//! then every alphanumeric character kept and every other character
//! folded to a hyphen run, with hyphen runs collapsed and leading
//! and trailing hyphens dropped. An empty result falls back to
//! `"section"`. [`dedup`] claims a slug against the document's used
//! anchor set, appending `-2`, `-3`, and so on until the claim is
//! unique — the GitHub convention, made fully deterministic by
//! processing headings in document order.

use std::collections::BTreeSet;

use unicode_normalization::UnicodeNormalization;

/// The outcome of inspecting a heading's trailing `{#...}` suffix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ExplicitId {
    /// No trailing `{#...}` suffix.
    None,
    /// A valid explicit id.
    Valid {
        /// The heading text before the suffix, trimmed.
        label_prefix: String,
        /// The explicit id.
        id: String,
    },
    /// A trailing `{#...}` suffix whose contents are not a valid id.
    Malformed {
        /// The raw contents between the braces.
        raw: String,
    },
}

/// Inspect `text` (the assembled raw heading text) for a trailing
/// `{#...}` explicit-id suffix.
pub(super) fn extract_explicit_id(text: &str) -> ExplicitId {
    let trimmed = text.trim_end();
    let Some(before_close) = trimmed.strip_suffix('}') else {
        return ExplicitId::None;
    };
    let Some(open) = before_close.rfind("{#") else {
        return ExplicitId::None;
    };
    let raw = &before_close[open + 2..];
    if is_valid_id(raw) {
        ExplicitId::Valid {
            label_prefix: before_close[..open].trim_end().to_string(),
            id: raw.to_string(),
        }
    } else {
        ExplicitId::Malformed {
            raw: raw.to_string(),
        }
    }
}

/// A valid explicit id: non-empty, ASCII alphanumeric first, then
/// ASCII alphanumerics, `-`, or `_`.
fn is_valid_id(raw: &str) -> bool {
    !raw.is_empty()
        && raw
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_alphanumeric())
        && raw
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Map heading text to its slug under the rules in the module docs.
/// Shared with the HTML adapter, which slugs id-less headings
/// identically.
pub(crate) fn slugify(text: &str) -> String {
    let nfc: String = text.nfc().collect();
    let mut out = String::new();
    let mut pending_hyphen = false;
    for ch in nfc.chars().flat_map(char::to_lowercase) {
        if ch.is_alphanumeric() {
            if pending_hyphen && !out.is_empty() {
                out.push('-');
            }
            pending_hyphen = false;
            out.push(ch);
        } else {
            pending_hyphen = true;
        }
    }
    if out.is_empty() {
        out.push_str("section");
    }
    out
}

/// Claim `slug` against the document's used anchor set, appending
/// `-2`, `-3`, and so on until the claim is unique. Shared with the
/// HTML adapter.
pub(crate) fn dedup(used: &mut BTreeSet<String>, slug: String) -> String {
    if used.insert(slug.clone()) {
        return slug;
    }
    let mut counter = 2u32;
    loop {
        let candidate = format!("{slug}-{counter}");
        if used.insert(candidate.clone()) {
            return candidate;
        }
        counter += 1;
    }
}
