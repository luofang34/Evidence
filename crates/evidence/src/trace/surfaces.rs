//! Catalog of user-visible surfaces the trace layer can claim.
//!
//! Each entry in [`KNOWN_SURFACES`] is a CLI verb (e.g. `"check"`,
//! `"schema show"`) or a named observable contract (e.g.
//! `"jsonl stream per Rule 2"`). HLR entries claim one or more
//! surfaces via [`HlrEntry::surfaces`](crate::trace::HlrEntry::surfaces);
//! the Link-phase validator asserts the bijection:
//!
//! - Every `KNOWN_SURFACES` entry is claimed by at least one HLR
//!   (otherwise `TRACE_HLR_SURFACE_UNCLAIMED`).
//! - Every `HlrEntry.surfaces` string is in `KNOWN_SURFACES`
//!   (otherwise `TRACE_HLR_SURFACE_UNKNOWN`).
//!
//! Adding a new subcommand or observable contract requires (a)
//! adding it here, and (b) claiming it on at least one HLR in the
//! same PR. The friction is intentional: it forces the requirement-
//! to-behavior linkage to be reviewer-visible.
//!
//! PR #49 / LLR-038.

/// Hand-curated catalog of known surfaces. Sorted alphabetically for
/// deterministic iteration order.
///
/// **Not yet covered**: `cargo evidence diff`, `init`,
/// `schema show`, `schema validate`. These subcommands exist but
/// don't have governing HLRs in the self-trace today; adding them
/// to KNOWN_SURFACES would fire `TRACE_HLR_SURFACE_UNCLAIMED`
/// immediately. The gap is itself the point of this bijection —
/// a follow-up PR adds HLRs for these subcommands (tracked in
/// tool/trace/README journal).
pub const KNOWN_SURFACES: &[&str] = &[
    // Sorted alphabetically (ASCII byte order: capitals before
    // lowercase). Named observable contracts start with capitals;
    // CLI verb names are lowercase.
    "CLI_SUBCOMMAND_ERROR terminal",
    "VERIFY_OK / VERIFY_FAIL / VERIFY_ERROR terminal contract",
    "check",
    "diagnostic code namespace (regex + reserved suffixes)",
    "floors",
    "generate",
    "jsonl stream per Schema Rule 2",
    "root_cause_uid grouping per Schema Rule 7",
    "rules",
    "trace",
    "verify",
];

#[cfg(test)]
mod tests {
    use super::*;

    /// `KNOWN_SURFACES` must stay sorted so the bijection report is
    /// deterministic and diff-reviewable.
    #[test]
    fn known_surfaces_is_sorted() {
        for pair in KNOWN_SURFACES.windows(2) {
            assert!(
                pair[0] < pair[1],
                "KNOWN_SURFACES out of order: '{}' must come before '{}'",
                pair[1],
                pair[0]
            );
        }
    }

    /// No duplicates.
    #[test]
    fn known_surfaces_is_unique() {
        use std::collections::BTreeSet;
        let set: BTreeSet<&&str> = KNOWN_SURFACES.iter().collect();
        assert_eq!(
            set.len(),
            KNOWN_SURFACES.len(),
            "duplicate entry in KNOWN_SURFACES"
        );
    }
}
