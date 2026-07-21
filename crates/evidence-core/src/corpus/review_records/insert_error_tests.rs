//! Tests for lifting `CorpusGraph::insert` failures into the review
//! error taxonomy (TEST-133).

use crate::corpus::{CorpusError, EdgeKind, ReviewError};

const REV_1: &str = "rev_00000000-0000-4000-8000-0000000000a1";
const REV_2: &str = "rev_00000000-0000-4000-8000-0000000000a2";

/// `ReviewError::from_insert` lifts the closed `CorpusGraph::insert`
/// error contract field for field and preserves any out-of-contract
/// variant whole as a typed source instead of panicking (TEST-133).
/// No public input reaches the preservation arm — `insert` fails
/// only on identity collisions and duplicate edges — so the mapping
/// is exercised directly with a synthetic `CorpusError`.
#[test]
fn insert_errors_lift_into_review_errors_without_panicking() {
    let out_of_contract = CorpusError::DanglingEdge {
        from: REV_1.to_string(),
        to: REV_2.to_string(),
        kind: EdgeKind::Supersedes,
    };
    let lifted = ReviewError::from_insert(out_of_contract);
    let source = match &lifted {
        ReviewError::UnexpectedInsertError(source) => Some(source),
        _ => None,
    }
    .expect("out-of-contract insert errors must be preserved");
    assert!(
        matches!(
            source.as_ref(),
            CorpusError::DanglingEdge {
                kind: EdgeKind::Supersedes,
                ..
            }
        ),
        "the original error is carried whole as the typed source: {source:?}"
    );
    assert_eq!(
        lifted.to_string(),
        format!("unexpected graph insertion error: {source}")
    );
    let exposed = std::error::Error::source(&lifted).expect("the boxed source is exposed");
    assert_eq!(exposed.to_string(), source.to_string());

    let in_contract = ReviewError::from_insert(CorpusError::DuplicateUid {
        uid: REV_1.to_string(),
    });
    assert!(
        matches!(in_contract, ReviewError::DuplicateUid { ref uid } if uid == REV_1),
        "in-contract variants still lift field for field: {in_contract:?}"
    );
}
