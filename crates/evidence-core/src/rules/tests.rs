//! Unit tests for `evidence_core::rules`. Lives in a sibling file pulled in
//! via `#[path]` from the parent so `rules.rs` stays under the
//! workspace 500-line file-size limit.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use super::*;
use std::collections::BTreeSet;

#[test]
fn rules_is_alphabetically_sorted() {
    for pair in RULES.windows(2) {
        assert!(
            pair[0].code < pair[1].code,
            "RULES out of order: '{}' must come before '{}'",
            pair[1].code,
            pair[0].code
        );
    }
}

#[test]
fn rules_codes_are_unique() {
    let set: BTreeSet<&str> = RULES.iter().map(|r| r.code).collect();
    assert_eq!(set.len(), RULES.len(), "duplicate code in RULES");
}

#[test]
fn rules_domain_matches_prefix() {
    for r in RULES {
        let derived = Domain::from_code(r.code)
            .unwrap_or_else(|| panic!("RULES entry '{}' has no recognizable prefix", r.code));
        assert_eq!(
            derived, r.domain,
            "RULES entry '{}' declares domain {:?} but its prefix implies {:?}",
            r.code, r.domain, derived
        );
    }
}

#[test]
fn terminal_entries_end_in_reserved_suffix() {
    for r in RULES.iter().filter(|r| r.terminal) {
        assert!(
            r.code.ends_with("_OK") || r.code.ends_with("_FAIL") || r.code.ends_with("_ERROR"),
            "terminal=true entry '{}' lacks reserved suffix",
            r.code
        );
    }
}

#[test]
fn cli_hand_emitted_set_is_disjoint_from_terminals() {
    let terminals: BTreeSet<&str> = crate::TERMINAL_CODES.iter().copied().collect();
    for c in HAND_EMITTED_CLI_CODES {
        assert!(
            !terminals.contains(c),
            "HAND_EMITTED_CLI_CODES entry '{}' must not also be in TERMINAL_CODES",
            c
        );
    }
}

#[test]
fn rules_json_parses_round_trip() {
    let json = rules_json();
    let v: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
    let arr = v.as_array().expect("top-level array");
    assert_eq!(arr.len(), RULES.len());
    for (i, entry) in arr.iter().enumerate() {
        let obj = entry.as_object().expect("entry is object");
        assert_eq!(
            obj["code"].as_str().unwrap(),
            RULES[i].code,
            "entry {} code mismatch",
            i
        );
        assert!(obj.contains_key("severity"));
        assert!(obj.contains_key("domain"));
        assert!(obj.contains_key("has_fix_hint"));
        assert!(obj.contains_key("terminal"));
    }
}
