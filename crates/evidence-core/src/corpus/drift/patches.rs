//! Patch, review, and candidate-effective planes of the drift
//! comparison (LLR-178).
//!
//! The two patch planes compare by patch uid: committed-only is
//! [`DriftCategory::PatchRemoved`], candidate-only is
//! [`DriftCategory::PatchAdded`], a moved reviewed-content digest
//! is [`DriftCategory::PatchChanged`]. Under an unchanged digest
//! the evaluated lifecycles compare: a state difference is
//! [`DriftCategory::ReviewStateChanged`], and the candidate-plane
//! state reports [`DriftCategory::PatchStale`] or
//! [`DriftCategory::PatchRejected`]. Per-item malformation never
//! aborts the plane: a candidate record whose reviewed-content
//! digest does not recompute, a uid carried by two distinct
//! candidate records, and an approved patch that cannot apply
//! against the candidate graph each degrade to their own
//! [`DriftCategory::PatchUnappliable`] finding while later patches
//! still report.
//!
//! The candidate effective graph applies exactly the
//! candidate-approved patches in uid order over the candidate
//! parser graph through the atomic application contract —
//! candidate, rejected, and stale patches never contribute, and a
//! failed approved patch is excluded after its finding. Only
//! patches present in both planes participate: a candidate-only
//! patch reports `PatchAdded` and never applies, while a
//! committed-only patch reports `PatchRemoved` and still
//! contributes to the committed effective graph when approved, so
//! removing an approved patch reports effective-graph drift. The
//! committed graph is never touched; application works on a clone.

use std::collections::{BTreeMap, BTreeSet};

use super::super::digest::StructuralContentDigest;
use super::super::patch_lifecycle::{PatchLifecycle, PatchLifecycleEvaluation};
use super::super::source_graph::SourceGraph;
use super::super::source_patch::apply::{PatchBindings, apply_patch};
use super::super::source_patch::digest::reviewed_content_digest;
use super::super::source_patch::records::SourcePatchRecord;
use super::findings::{DriftCategory, DriftDetail, DriftFinding};

/// The patch-plane comparison result: the findings and the
/// candidate effective graph (the candidate parser graph plus
/// exactly the cleanly applied candidate-approved patches).
pub(super) struct PatchPlaneOutcome {
    /// The patch and review findings, unsorted (the caller sorts).
    pub findings: Vec<DriftFinding>,
    /// The candidate effective graph.
    pub effective: SourceGraph,
}

/// The candidate side's patch-plane inputs.
pub(super) struct CandidatePatchPlane<'p> {
    /// The candidate parser graph the effective graph builds over.
    pub graph: &'p SourceGraph,
    /// The candidate plane's patches bound to the compared
    /// revision.
    pub patches: &'p [SourcePatchRecord],
    /// The candidate plane's patch lifecycle evaluations.
    pub evaluations: &'p BTreeMap<String, PatchLifecycleEvaluation>,
    /// The candidate recipe identity digest, when presented.
    pub recipe_digest: Option<&'p StructuralContentDigest>,
    /// The candidate verified-input digest, when presented.
    pub input_digest: Option<&'p StructuralContentDigest>,
}

/// Compare the patch and review planes of both sides and build the
/// candidate effective graph. `committed` and `candidate` carry
/// only patches bound to the compared revision, and every patch of
/// each plane carries an evaluation (the caller's prerequisite).
pub(super) fn compare_patches(
    media_type: &str,
    committed: &BTreeMap<String, SourcePatchRecord>,
    committed_evaluations: &BTreeMap<String, PatchLifecycleEvaluation>,
    candidate: &CandidatePatchPlane,
) -> PatchPlaneOutcome {
    let mut findings = Vec::new();
    let candidate_by_uid = group_candidate_uids(candidate.patches, &mut findings);
    let uids: BTreeSet<&str> = committed
        .keys()
        .map(String::as_str)
        .chain(candidate_by_uid.keys().map(String::as_str))
        .collect();
    let mut effective = candidate.graph.clone();
    for uid in uids {
        match (committed.get(uid), candidate_by_uid.get(uid)) {
            (Some(_), None) => findings.push(patch_finding(
                DriftCategory::PatchRemoved,
                uid,
                DriftDetail::CommittedOnly,
            )),
            (None, Some(_)) => findings.push(patch_finding(
                DriftCategory::PatchAdded,
                uid,
                DriftDetail::CandidateOnly,
            )),
            (Some(committed_patch), Some(candidate_patch)) => {
                if candidate_patch.reviewed_content_digest
                    != reviewed_content_digest(candidate_patch)
                {
                    findings.push(patch_finding(
                        DriftCategory::PatchUnappliable,
                        uid,
                        DriftDetail::Failure {
                            message: "candidate patch record's reviewed-content digest does not \
                                      recompute from its bindings and operations"
                                .to_string(),
                        },
                    ));
                    continue;
                }
                if candidate_patch.reviewed_content_digest
                    != committed_patch.reviewed_content_digest
                {
                    findings.push(patch_finding(
                        DriftCategory::PatchChanged,
                        uid,
                        DriftDetail::PatchDigest {
                            committed: committed_patch.reviewed_content_digest.as_str().to_string(),
                            candidate: candidate_patch.reviewed_content_digest.as_str().to_string(),
                        },
                    ));
                    continue;
                }
                let (Some(committed_eval), Some(candidate_eval)) = (
                    committed_evaluations.get(uid),
                    candidate.evaluations.get(uid),
                ) else {
                    continue;
                };
                if candidate_eval.state != committed_eval.state {
                    findings.push(patch_finding(
                        DriftCategory::ReviewStateChanged,
                        uid,
                        DriftDetail::LifecycleStates {
                            committed: state_name(committed_eval.state).to_string(),
                            candidate: state_name(candidate_eval.state).to_string(),
                        },
                    ));
                }
                if candidate_eval.state == PatchLifecycle::Stale {
                    findings.push(patch_finding(
                        DriftCategory::PatchStale,
                        uid,
                        DriftDetail::LifecycleStates {
                            committed: state_name(committed_eval.state).to_string(),
                            candidate: state_name(candidate_eval.state).to_string(),
                        },
                    ));
                }
                if candidate_eval.state == PatchLifecycle::Rejected {
                    findings.push(patch_finding(
                        DriftCategory::PatchRejected,
                        uid,
                        DriftDetail::LifecycleStates {
                            committed: state_name(committed_eval.state).to_string(),
                            candidate: state_name(candidate_eval.state).to_string(),
                        },
                    ));
                }
                if candidate_eval.state == PatchLifecycle::Approved {
                    apply_approved(
                        &mut effective,
                        candidate_patch,
                        media_type,
                        candidate.recipe_digest,
                        candidate.input_digest,
                        &mut findings,
                    );
                }
            }
            (None, None) => {}
        }
    }
    PatchPlaneOutcome {
        findings,
        effective,
    }
}

/// Group candidate patches by uid in uid order. A uid carried by
/// two distinct records is malformed plane data: one deterministic
/// finding (the group is sorted by reviewed-content digest first,
/// so the check is order-independent) and the uid is excluded.
fn group_candidate_uids<'p>(
    candidate: &'p [SourcePatchRecord],
    findings: &mut Vec<DriftFinding>,
) -> BTreeMap<String, &'p SourcePatchRecord> {
    let mut groups: BTreeMap<&str, Vec<&SourcePatchRecord>> = BTreeMap::new();
    for patch in candidate {
        groups.entry(patch.uid.as_str()).or_default().push(patch);
    }
    let mut by_uid = BTreeMap::new();
    for (uid, mut group) in groups {
        group.sort_by(|a, b| {
            a.reviewed_content_digest
                .as_str()
                .cmp(b.reviewed_content_digest.as_str())
        });
        let distinct = group
            .windows(2)
            .any(|pair| pair[0].reviewed_content_digest != pair[1].reviewed_content_digest);
        if distinct {
            findings.push(patch_finding(
                DriftCategory::PatchUnappliable,
                uid,
                DriftDetail::Failure {
                    message: format!(
                        "candidate plane carries {} distinct patch records with uid {uid}",
                        group.len()
                    ),
                },
            ));
            continue;
        }
        if let Some(patch) = group.first() {
            by_uid.insert(uid.to_string(), *patch);
        }
    }
    by_uid
}

/// Apply one candidate-approved patch to the effective working
/// graph; a failure degrades to its own finding and leaves the
/// working graph untouched.
fn apply_approved(
    effective: &mut SourceGraph,
    patch: &SourcePatchRecord,
    media_type: &str,
    recipe_digest: Option<&StructuralContentDigest>,
    input_digest: Option<&StructuralContentDigest>,
    findings: &mut Vec<DriftFinding>,
) {
    let (Some(recipe_digest), Some(input_digest)) = (recipe_digest, input_digest) else {
        findings.push(patch_finding(
            DriftCategory::PatchUnappliable,
            &patch.uid,
            DriftDetail::Failure {
                message: "candidate recipe or input identity is unavailable; the patch's \
                          bindings are unverifiable"
                    .to_string(),
            },
        ));
        return;
    };
    let bindings = PatchBindings {
        recipe_digest: recipe_digest.clone(),
        input_digest: input_digest.clone(),
    };
    match apply_patch(effective, patch, &bindings, media_type) {
        Ok(application) => *effective = application.graph,
        Err(source) => findings.push(patch_finding(
            DriftCategory::PatchUnappliable,
            &patch.uid,
            DriftDetail::Failure {
                message: source.to_string(),
            },
        )),
    }
}

fn patch_finding(category: DriftCategory, patch_uid: &str, detail: DriftDetail) -> DriftFinding {
    DriftFinding {
        category,
        structural_path: None,
        node_uid: None,
        patch_uid: Some(patch_uid.to_string()),
        detail,
    }
}

fn state_name(state: PatchLifecycle) -> &'static str {
    match state {
        PatchLifecycle::Candidate => "candidate",
        PatchLifecycle::Approved => "approved",
        PatchLifecycle::Rejected => "rejected",
        PatchLifecycle::Stale => "stale",
    }
}
