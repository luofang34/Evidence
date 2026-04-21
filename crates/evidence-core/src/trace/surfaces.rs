//! Catalog of user-visible surfaces the trace layer can claim.
//!
//! Each entry in [`KNOWN_SURFACES`] is a CLI verb (e.g. `"check"`,
//! `"schema show"`) or a named observable contract (e.g.
//! `"jsonl stream per Rule 2"`). HLR entries claim one or more
//! surfaces via [`HlrEntry::surfaces`](crate::trace::HlrEntry::surfaces);
//! the Link-phase validator asserts the bijection:
//!
//! - Every `KNOWN_SURFACES` entry is claimed by at least one HLR
//!   (otherwise the validator reports "not claimed by any HLR").
//! - Every `HlrEntry.surfaces` string is in `KNOWN_SURFACES`
//!   (otherwise the validator reports "not in KNOWN_SURFACES").
//!
//! Adding a new subcommand or observable contract requires (a)
//! adding it here, and (b) claiming it on at least one HLR in the
//! same PR. The friction is intentional: it forces the requirement-
//! to-behavior linkage to be reviewer-visible.
//!
//! LLR-038.

/// Hand-curated catalog of known surfaces. Two groups, each sorted
/// alphabetically within its group; groups separated by a comment
/// line for reviewer readability. The `known_surfaces_is_sorted`
/// unit test asserts *within-group* order, not cross-group order, so
/// the two groups can use their natural conventions (lowercase CLI
/// verbs; Capitalized observable-contract labels).
///
/// **Not yet covered**: `cargo evidence diff`, `init`,
/// `schema show`, `schema validate`. These subcommands exist but
/// don't have governing HLRs in the self-trace today; adding them
/// to KNOWN_SURFACES would fire the unclaimed-surface rule
/// immediately. The gap is itself the point of this bijection —
/// a follow-up PR adds HLRs for these subcommands (tracked in
/// tool/trace/README journal).
pub const KNOWN_SURFACES: &[&str] = &[
    // Group 1 — CLI verb names (lowercase; match the `Commands::*`
    // variants exactly).
    "check",
    "doctor",
    "floors",
    "generate",
    "rules",
    "trace",
    "verify",
    // Group 2 — named observable contracts (capitalized prose; match
    // terminology used in schemas/diagnostic.schema.json).
    "CLI_SUBCOMMAND_ERROR terminal",
    "VERIFY_OK / VERIFY_FAIL / VERIFY_ERROR terminal contract",
    "agent MCP surface",
    "comment hygiene gate",
    "diagnostic code namespace (regex + reserved suffixes)",
    "jsonl stream per Schema Rule 2",
    "pre-release safety gate",
    "root_cause_uid grouping per Schema Rule 7",
    "structured Link-phase diagnostic codes",
    "trace-ID reference bijection",
    "walker-usage standardization",
];

/// Index of the first entry in the contracts group — everything at
/// `KNOWN_SURFACES[..CONTRACTS_START]` is a verb; everything at
/// `KNOWN_SURFACES[CONTRACTS_START..]` is an observable contract.
/// Used by the group-scoped sort test to validate within-group order
/// without imposing cross-group ordering.
#[cfg(test)]
const CONTRACTS_START: usize = 7;

#[cfg(test)]
mod tests {
    use super::*;

    /// Within each group, entries must stay sorted so the bijection
    /// report is deterministic and diff-reviewable. Cross-group order
    /// is fixed by the group split, not by ASCII sort — that way
    /// lowercase verbs and capitalized contract labels don't interleave
    /// visually.
    #[test]
    fn known_surfaces_is_sorted() {
        let verbs = &KNOWN_SURFACES[..CONTRACTS_START];
        for pair in verbs.windows(2) {
            assert!(
                pair[0] < pair[1],
                "verbs group out of order: '{}' must come before '{}'",
                pair[1],
                pair[0]
            );
        }
        let contracts = &KNOWN_SURFACES[CONTRACTS_START..];
        for pair in contracts.windows(2) {
            assert!(
                pair[0] < pair[1],
                "contracts group out of order: '{}' must come before '{}'",
                pair[1],
                pair[0]
            );
        }
    }

    /// No duplicates anywhere in the catalog.
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

    /// Spot-check the group split: the verb-group's last entry must
    /// be in the CLI-verb allowlist, and the contracts-group's first
    /// entry must not. Keeps the split honest without trying to
    /// auto-detect group membership from capitalization (which fails
    /// for labels like "diagnostic code namespace …" that describe a
    /// contract in lowercase prose).
    #[test]
    fn contracts_start_index_is_consistent() {
        const CLI_VERBS: &[&str] = &[
            "check",
            "doctor",
            "floors",
            "generate",
            "rules",
            "trace",
            "verify",
            "diff",
            "init",
            "schema show",
            "schema validate",
        ];
        assert!(
            CLI_VERBS.contains(&KNOWN_SURFACES[CONTRACTS_START - 1]),
            "verb group boundary entry '{}' is not in CLI_VERBS allowlist",
            KNOWN_SURFACES[CONTRACTS_START - 1]
        );
        assert!(
            !CLI_VERBS.contains(&KNOWN_SURFACES[CONTRACTS_START]),
            "contracts group boundary entry '{}' is a CLI verb, not a contract",
            KNOWN_SURFACES[CONTRACTS_START]
        );
    }
}
