//! `StandardsPack` — the versioned, expert-reviewed binding between
//! the tool's objective mapping and the standard it claims (LLR-110).
//!
//! Every compliance report records the pack so an auditor can see
//! exactly which mapping revision produced the verdicts and where
//! its applicability/tailoring review lives. The pack is a
//! code-level constant — bumping the mapping means bumping
//! `version` in the same commit as the review record update.

use serde::Serialize;

use super::assurance::AssuranceLevel;
use crate::compliance::{Applicability, OBJECTIVES};

/// A versioned standards pack: identity, revision, and the review
/// record an auditor can cross-reference for the applicability and
/// tailoring decisions behind the objective mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct StandardsPack {
    /// Pack identifier (`"do-178c-ac20-115d"`).
    pub id: &'static str,
    /// Pack revision. Bumped in the same commit as any objective-
    /// mapping change, with the review record updated alongside.
    pub version: &'static str,
    /// Pointer to the applicability, tailoring, and
    /// tool-qualification review record for this mapping.
    pub review_record: &'static str,
}

impl StandardsPack {
    /// The pack this release binds: DO-178C as accepted by
    /// FAA AC 20-115D.
    pub fn do_178c() -> Self {
        DO_178C_PACK
    }
}

/// The DO-178C pack constant. `review_record` points at
/// `cert/QUALIFICATION.md`, which carries the A-7 applicability and
/// tailoring rationale plus the tool-qualification gap statements.
pub const DO_178C_PACK: StandardsPack = StandardsPack {
    id: "do-178c-ac20-115d",
    version: "1.0.0",
    review_record: "cert/QUALIFICATION.md — DO-178C applicability, tailoring, and tool-qualification review record",
};

/// Table A-7 applicability per objective at `level`, delegating to
/// the objectives table — the mapping is not duplicated here. This
/// is the machine-readable form of the pack's applicability record.
pub fn a7_applicability(level: AssuranceLevel) -> Vec<(&'static str, Applicability)> {
    OBJECTIVES
        .iter()
        .filter(|o| o.table == "Table A-7")
        .map(|o| (o.id, o.applicability_for(level.effective_policy_dal())))
        .collect()
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
    use crate::policy::Dal;

    #[test]
    fn pack_binds_id_version_and_review_record() {
        let pack = StandardsPack::do_178c();
        assert_eq!(pack.id, "do-178c-ac20-115d");
        assert_eq!(pack.version, "1.0.0");
        assert!(pack.review_record.contains("cert/QUALIFICATION.md"));
        let json = serde_json::to_value(pack).unwrap();
        assert_eq!(json["id"], "do-178c-ac20-115d");
        assert_eq!(json["version"], "1.0.0");
    }

    /// The accessor must delegate to the objectives table, not a
    /// copy: for every Table A-7 objective the returned applicability
    /// equals `applicability_for` at the level's policy row.
    #[test]
    fn a7_applicability_delegates_to_objectives_table() {
        for level in [
            AssuranceLevel::DalA,
            AssuranceLevel::DalB,
            AssuranceLevel::DalC,
            AssuranceLevel::DalD,
            AssuranceLevel::Unclassified,
        ] {
            let rows = a7_applicability(level);
            assert!(!rows.is_empty());
            for (id, app) in &rows {
                let obj = OBJECTIVES.iter().find(|o| &o.id == id).unwrap();
                assert_eq!(obj.table, "Table A-7");
                assert_eq!(
                    *app,
                    obj.applicability_for(level.effective_policy_dal()),
                    "applicability drift for {id} at {level:?}"
                );
            }
        }
        // Unclassified uses the least-strict row: identical to DAL-D.
        assert_eq!(
            a7_applicability(AssuranceLevel::Unclassified),
            OBJECTIVES
                .iter()
                .filter(|o| o.table == "Table A-7")
                .map(|o| (o.id, o.applicability_for(Dal::D)))
                .collect::<Vec<_>>()
        );
    }
}
