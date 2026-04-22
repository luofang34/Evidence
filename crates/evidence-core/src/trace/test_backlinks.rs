//! Per-test → LLR back-link resolver. Fills
//! [`crate::bundle::TestOutcomeRecord::requirement_uids`] by
//! joining each test's `{module_path}::{name}` against every
//! [`TestEntry`]'s `test_selectors` (prefix or full-match),
//! then collecting the matched `TestEntry.traces_to` LLR-UIDs
//! into the record.
//!
//! This is the forward direction of the traceability loop
//! (requirement → test → back to requirement). The reverse
//! check — every LLR's test_selectors resolves to at least one
//! test — belongs in `verify` (follow-up PR).
//!
//! The resolver runs at generate time after the trace phase so
//! the enriched outcomes land in `tests/test_outcomes.jsonl`
//! inside the content layer (SHA256SUMS-integrity-covered).

use crate::bundle::TestOutcomeRecord;
use crate::trace::entries::TestEntry;

/// For each `record` in `records`, set
/// `record.requirement_uids` to the deduplicated union of
/// `traces_to` across every [`TestEntry`] whose `test_selectors`
/// match the record's qualified name.
///
/// Matching rule: a selector matches iff the selector is either
/// exactly equal to `"{module_path}::{name}"` OR is a module-
/// path prefix (a selector without the test fn suffix matches
/// every test under that module). The rule mirrors
/// [`crate::trace::resolve_test_selectors`]'s resolver
/// convention.
///
/// Overwrites any existing `requirement_uids` values on the
/// records (idempotent on re-run).
pub fn resolve_llr_backlinks(records: &mut [TestOutcomeRecord], tests: &[TestEntry]) {
    for record in records.iter_mut() {
        let qualified = if record.module_path.is_empty() {
            record.name.clone()
        } else {
            format!("{}::{}", record.module_path, record.name)
        };
        let mut uids: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for test_entry in tests {
            if selector_matches_qualified(&test_entry.all_selectors(), &qualified) {
                for uid in &test_entry.traces_to {
                    uids.insert(uid.clone());
                }
            }
        }
        record.requirement_uids = uids.into_iter().collect();
    }
}

/// `true` iff any selector in `selectors` either equals
/// `qualified` or is a `::`-boundary prefix of it. A bare
/// prefix like `"evidence_core::env::capture"` matches every
/// `evidence_core::env::capture::*::fn` under it, but NOT a
/// sibling module `evidence_core::env::capture_other`.
fn selector_matches_qualified(selectors: &[String], qualified: &str) -> bool {
    for sel in selectors {
        if sel == qualified {
            return true;
        }
        // Prefix match on `::` boundary.
        if let Some(rest) = qualified.strip_prefix(sel.as_str())
            && rest.starts_with("::")
        {
            return true;
        }
    }
    false
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
mod tests {
    use super::*;
    use crate::trace::entries::Schema;

    fn test_entry(uid: &str, selectors: &[&str], traces_to: &[&str]) -> TestEntry {
        TestEntry {
            uid: Some(uid.to_string()),
            ns: None,
            id: format!("TEST-{uid}"),
            title: "title".to_string(),
            owner: None,
            sort_key: None,
            traces_to: traces_to.iter().map(|s| s.to_string()).collect(),
            description: None,
            category: None,
            test_selector: None,
            test_selectors: selectors.iter().map(|s| s.to_string()).collect(),
            source: None,
        }
    }

    fn record(module_path: &str, name: &str) -> TestOutcomeRecord {
        TestOutcomeRecord {
            name: name.to_string(),
            module_path: module_path.to_string(),
            passed: true,
            ignored: false,
            failure_message: None,
            duration_ms: None,
            requirement_uids: Vec::new(),
        }
    }

    // Silence unused-import warning when Schema isn't
    // referenced in this module (keeps the import visible for
    // future test-entry field additions).
    #[allow(dead_code)]
    const _SCHEMA: Option<Schema> = None;

    /// Exact-match: selector equals `{module_path}::{name}`.
    #[test]
    fn exact_match_populates_uids() {
        let te = test_entry("t-1", &["foo::bar::my_fn"], &["llr-a"]);
        let mut recs = vec![record("foo::bar", "my_fn")];
        resolve_llr_backlinks(&mut recs, &[te]);
        assert_eq!(recs[0].requirement_uids, vec!["llr-a".to_string()]);
    }

    /// Module-path prefix matches every test under it.
    #[test]
    fn prefix_match_populates_uids() {
        let te = test_entry("t-1", &["foo::bar"], &["llr-b"]);
        let mut recs = vec![record("foo::bar", "f1"), record("foo::bar::sub", "f2")];
        resolve_llr_backlinks(&mut recs, &[te]);
        assert_eq!(recs[0].requirement_uids, vec!["llr-b".to_string()]);
        assert_eq!(recs[1].requirement_uids, vec!["llr-b".to_string()]);
    }

    /// Sibling-module paths MUST NOT match.
    #[test]
    fn sibling_module_is_not_a_prefix_match() {
        // Selector "foo::bar" must not match test
        // "foo::barn::my_fn" — that would be substring-not-
        // path-boundary matching.
        let te = test_entry("t-1", &["foo::bar"], &["llr-b"]);
        let mut recs = vec![record("foo::barn", "my_fn")];
        resolve_llr_backlinks(&mut recs, &[te]);
        assert!(recs[0].requirement_uids.is_empty());
    }

    /// Multiple matching tests entries dedupe their
    /// traces_to union into the record.
    #[test]
    fn multiple_entries_dedupe_union() {
        let te1 = test_entry("t-1", &["foo"], &["llr-a", "llr-b"]);
        let te2 = test_entry("t-2", &["foo::bar"], &["llr-b", "llr-c"]);
        let mut recs = vec![record("foo::bar", "fn1")];
        resolve_llr_backlinks(&mut recs, &[te1, te2]);
        // BTreeSet yields sorted order.
        assert_eq!(
            recs[0].requirement_uids,
            vec![
                "llr-a".to_string(),
                "llr-b".to_string(),
                "llr-c".to_string()
            ]
        );
    }

    /// No matching TestEntry → empty requirement_uids (legitimate
    /// harness / setup test, not a bug).
    #[test]
    fn no_match_leaves_uids_empty() {
        let te = test_entry("t-1", &["other::module"], &["llr-x"]);
        let mut recs = vec![record("foo::bar", "my_fn")];
        resolve_llr_backlinks(&mut recs, &[te]);
        assert!(recs[0].requirement_uids.is_empty());
    }

    /// Empty `module_path` (integration test at binary root) —
    /// qualified name is just `name`; selector `"test_name"`
    /// matches exactly.
    #[test]
    fn empty_module_path_uses_bare_name() {
        let te = test_entry("t-1", &["integration_test_fn"], &["llr-z"]);
        let mut recs = vec![record("", "integration_test_fn")];
        resolve_llr_backlinks(&mut recs, &[te]);
        assert_eq!(recs[0].requirement_uids, vec!["llr-z".to_string()]);
    }
}
