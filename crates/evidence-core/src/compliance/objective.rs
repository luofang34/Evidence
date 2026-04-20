//! `Objective` — a single DO-178C Annex A row and its per-DAL applicability.

use serde::{Deserialize, Serialize};

use crate::policy::Dal;

use super::applicability::Applicability;

/// A single DO-178C objective from Annex A tables.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Objective {
    /// Unique objective ID (e.g., "A3-1", "A7-3")
    pub id: &'static str,
    /// DO-178C table reference (e.g., "Table A-3")
    pub table: &'static str,
    /// Human-readable objective title
    pub title: &'static str,
    /// Applicability per DAL level: [A, B, C, D]
    pub applicability: [Applicability; 4],
}

impl Objective {
    /// Get applicability for a specific DAL level.
    pub fn applicability_for(&self, dal: Dal) -> Applicability {
        match dal {
            Dal::A => self.applicability[0],
            Dal::B => self.applicability[1],
            Dal::C => self.applicability[2],
            Dal::D => self.applicability[3],
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
mod tests {
    use super::super::objectives_table::OBJECTIVES;
    use super::*;

    #[test]
    fn test_applicability_dal_a() {
        // DAL-A: all objectives should be applicable
        for obj in OBJECTIVES {
            let app = obj.applicability_for(Dal::A);
            assert_ne!(
                app,
                Applicability::NotApplicable,
                "objective {} should be applicable at DAL-A",
                obj.id
            );
        }
    }

    #[test]
    fn test_applicability_dal_d_relaxed() {
        // DAL-D: several objectives should be not applicable
        let na_count = OBJECTIVES
            .iter()
            .filter(|obj| obj.applicability_for(Dal::D) == Applicability::NotApplicable)
            .count();
        assert!(
            na_count > 0,
            "DAL-D should have some non-applicable objectives"
        );
    }

    #[test]
    fn test_mcdc_only_dal_a() {
        // A7-10 (MC/DC) is only required at DAL-A
        let mcdc = OBJECTIVES.iter().find(|o| o.id == "A7-10").unwrap();
        assert_eq!(
            mcdc.applicability_for(Dal::A),
            Applicability::RequiredWithIndependence
        );
        assert_eq!(mcdc.applicability_for(Dal::B), Applicability::NotApplicable);
        assert_eq!(mcdc.applicability_for(Dal::C), Applicability::NotApplicable);
        assert_eq!(mcdc.applicability_for(Dal::D), Applicability::NotApplicable);
    }
}
