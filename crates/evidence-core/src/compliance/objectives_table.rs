//! `OBJECTIVES` — the canonical DO-178C Annex A Tables A-3 through A-7.

use super::applicability::Applicability;
use super::objective::Objective;

// Abbreviations for readability
use Applicability::{NotApplicable as NA, Required as R, RequiredWithIndependence as RI};

/// DO-178C Annex A objectives relevant to tool-automatable verification.
/// This covers Tables A-3 through A-7 (the tables where cargo-evidence can
/// provide evidence). Tables A-1, A-2, A-8 through A-10 are process/management
/// objectives that require human documentation, not tool output.
pub static OBJECTIVES: &[Objective] = &[
    // Table A-3: Verification of HLR
    Objective {
        id: "A3-1",
        table: "Table A-3",
        title: "HLR comply with system requirements",
        applicability: [RI, RI, R, R],
    },
    Objective {
        id: "A3-2",
        table: "Table A-3",
        title: "HLR are accurate and consistent",
        applicability: [RI, RI, R, R],
    },
    Objective {
        id: "A3-3",
        table: "Table A-3",
        title: "HLR are compatible with target computer",
        applicability: [RI, RI, R, R],
    },
    Objective {
        id: "A3-4",
        table: "Table A-3",
        title: "HLR are verifiable",
        applicability: [RI, RI, R, R],
    },
    Objective {
        id: "A3-5",
        table: "Table A-3",
        title: "HLR conform to standards",
        applicability: [RI, RI, R, R],
    },
    Objective {
        id: "A3-6",
        table: "Table A-3",
        title: "HLR are traceable to system requirements",
        applicability: [RI, RI, R, R],
    },
    Objective {
        id: "A3-7",
        table: "Table A-3",
        title: "Algorithms are accurate",
        applicability: [RI, RI, R, NA],
    },
    // Table A-4: Verification of LLR
    Objective {
        id: "A4-1",
        table: "Table A-4",
        title: "LLR comply with HLR",
        applicability: [RI, RI, R, R],
    },
    Objective {
        id: "A4-2",
        table: "Table A-4",
        title: "LLR are accurate and consistent",
        applicability: [RI, RI, R, R],
    },
    Objective {
        id: "A4-3",
        table: "Table A-4",
        title: "LLR are compatible with target computer",
        applicability: [RI, RI, R, R],
    },
    Objective {
        id: "A4-4",
        table: "Table A-4",
        title: "LLR are verifiable",
        applicability: [RI, RI, R, R],
    },
    Objective {
        id: "A4-5",
        table: "Table A-4",
        title: "LLR conform to standards",
        applicability: [RI, RI, R, R],
    },
    Objective {
        id: "A4-6",
        table: "Table A-4",
        title: "LLR are traceable to HLR",
        applicability: [RI, RI, R, R],
    },
    Objective {
        id: "A4-7",
        table: "Table A-4",
        title: "LLR algorithms are accurate",
        applicability: [RI, RI, R, NA],
    },
    Objective {
        id: "A4-8",
        table: "Table A-4",
        title: "LLR are compatible with target",
        applicability: [RI, RI, R, NA],
    },
    // Table A-5: Verification of Software Architecture
    Objective {
        id: "A5-1",
        table: "Table A-5",
        title: "Architecture is compatible with HLR",
        applicability: [RI, RI, R, NA],
    },
    Objective {
        id: "A5-2",
        table: "Table A-5",
        title: "Architecture is consistent",
        applicability: [RI, RI, R, NA],
    },
    Objective {
        id: "A5-3",
        table: "Table A-5",
        title: "Architecture is compatible with target",
        applicability: [RI, RI, R, NA],
    },
    Objective {
        id: "A5-4",
        table: "Table A-5",
        title: "Architecture is verifiable",
        applicability: [RI, RI, R, NA],
    },
    Objective {
        id: "A5-5",
        table: "Table A-5",
        title: "Architecture conforms to standards",
        applicability: [RI, RI, R, NA],
    },
    // Table A-6: Verification of Source Code
    Objective {
        id: "A6-1",
        table: "Table A-6",
        title: "Source code complies with LLR",
        applicability: [RI, RI, R, R],
    },
    Objective {
        id: "A6-2",
        table: "Table A-6",
        title: "Source code complies with architecture",
        applicability: [RI, RI, R, R],
    },
    Objective {
        id: "A6-3",
        table: "Table A-6",
        title: "Source code is verifiable",
        applicability: [RI, RI, R, NA],
    },
    Objective {
        id: "A6-4",
        table: "Table A-6",
        title: "Source code conforms to standards",
        applicability: [RI, RI, R, R],
    },
    Objective {
        id: "A6-5",
        table: "Table A-6",
        title: "Source code is traceable to LLR",
        applicability: [RI, RI, R, R],
    },
    // Table A-7: Verification of Integration (Testing)
    Objective {
        id: "A7-1",
        table: "Table A-7",
        title: "Executable object code complies with HLR",
        applicability: [RI, RI, R, R],
    },
    Objective {
        id: "A7-2",
        table: "Table A-7",
        title: "Executable object code is robust with HLR",
        applicability: [RI, RI, R, R],
    },
    Objective {
        id: "A7-3",
        table: "Table A-7",
        title: "Executable object code complies with LLR",
        applicability: [RI, RI, R, NA],
    },
    Objective {
        id: "A7-4",
        table: "Table A-7",
        title: "Executable object code is robust with LLR",
        applicability: [RI, RI, NA, NA],
    },
    Objective {
        id: "A7-5",
        table: "Table A-7",
        title: "Executable object code is compatible with target",
        applicability: [RI, RI, R, R],
    },
    Objective {
        id: "A7-6",
        table: "Table A-7",
        title: "Test coverage of HLR is achieved",
        applicability: [RI, RI, R, R],
    },
    Objective {
        id: "A7-7",
        table: "Table A-7",
        title: "Test coverage of LLR is achieved",
        applicability: [RI, RI, R, NA],
    },
    Objective {
        id: "A7-8",
        table: "Table A-7",
        title: "Test coverage of software structure (statement) is achieved",
        applicability: [RI, RI, R, NA],
    },
    Objective {
        id: "A7-9",
        table: "Table A-7",
        title: "Test coverage of software structure (decision) is achieved",
        applicability: [RI, RI, NA, NA],
    },
    Objective {
        id: "A7-10",
        table: "Table A-7",
        title: "Test coverage of software structure (MC/DC) is achieved",
        applicability: [RI, NA, NA, NA],
    },
];

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
mod tests {
    use super::*;

    #[test]
    fn test_objective_count() {
        // We encode Tables A-3 through A-7 (automatable subset)
        // A-3: 7, A-4: 8, A-5: 5, A-6: 5, A-7: 10 = 35
        assert_eq!(OBJECTIVES.len(), 35);
    }
}
