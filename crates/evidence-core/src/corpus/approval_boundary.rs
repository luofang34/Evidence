//! Strict approval boundary for implementation and verification
//! evidence (LLR-119, LLR-120, LLR-121).
//!
//! [`validate_approval_boundary`] is the policy-neutral validator
//! that keeps implementation or verification evidence from claiming
//! an unapproved requirement. Under explicit enforcement it gates
//! two kinds of claims against the evaluated lifecycle
//! ([`evaluate_all_lifecycles`]) of every target requirement:
//!
//! 1. **Verification** — every `Test` → `Requirement`
//!    [`EdgeKind::Verifies`] edge must target an
//!    [`RequirementLifecycle::Approved`] requirement.
//! 2. **Implementation** — every requirement whose trace metadata
//!    claims implementation modules (`modules`) or emitted
//!    implementation diagnostic codes (`emits`) must be `Approved`.
//!
//! [`EdgeKind::DerivesFrom`] decomposition edges are never gated:
//! candidate authoring of a requirement hierarchy stays usable
//! before approval. This validator gates implementation and
//! verification consumption, not the ability to draft a requirement
//! hierarchy.
//!
//! # Explicit enforcement, never inferred
//!
//! The policy is a function argument — [`LifecycleEnforcement`] —
//! with exactly one variant, no `Default`, and no serde
//! representation. Strictness is never inferred from an implicit
//! design-assurance-level fallback, and no environment variable or
//! configuration file can turn enforcement on: the caller names the
//! policy in code or the check does not run. The "not requested"
//! case is the absence of the call.
//!
//! # Compatibility boundary
//!
//! Legacy graphs are **not** grandfathered. A corpus with zero
//! review records evaluates every requirement as
//! [`RequirementLifecycle::Candidate`] — missing reviews are never
//! implicitly approved — so an explicitly requested approval claim
//! over zero reviews can never succeed: every would-be-gated claim
//! produces a violation. Zero reviews alone are not a failure,
//! though: under [`LifecycleEnforcement::Required`] an empty review
//! set fails closed **only when a gated claim exists** (a test
//! [`EdgeKind::Verifies`] edge, or a requirement metadata `modules`
//! or `emits` claim). A zero-review graph with no gated claims
//! yields `Ok(())` — enforcement gates claims, not the existence of
//! requirements. Callers that have not adopted corpus
//! lifecycle simply do not call this validator; the distinction is
//! explicit in the API and tests, never detected from file paths.
//! Making enforcement mandatory for certification and record corpus
//! claims is M6 cutover work above this layer; this module stays
//! policy-neutral.
//!
//! Native requirement records carry no `modules`/`emits` fields, so
//! the implementation-claim gate applies to claims present in the
//! graph (today: populated by the legacy trace adapter). Native
//! records expressing those claims are a documented non-goal here.
//!
//! # Diagnostics and determinism
//!
//! Every non-approved target produces one
//! [`ApprovalBoundaryViolation`] per claim, carrying the requirement
//! uid, the human-readable id, the lifecycle state (`Candidate`,
//! `Rejected`, or `Stale` — never `Approved`), and the
//! [`ReferringArtifact`]. Violations aggregate into a single
//! [`ApprovalBoundaryError::Violations`] — never first-fail — sorted
//! by requirement uid, then referring artifact
//! ([`ReferringArtifact::Test`] before
//! [`ReferringArtifact::ImplementationModules`] before
//! [`ReferringArtifact::EmittedDiagnostics`], payload fields breaking
//! ties). The error's `Display` renders one line per violation in
//! that order. A malformed graph never reaches the claim scan:
//! [`evaluate_all_lifecycles`] validates the graph first, and the
//! impossible-invariant failure (a dangling edge, a review-graph
//! invariant violation) fails closed as
//! [`ApprovalBoundaryError::Lifecycle`] with the typed source chain
//! — [`LifecycleError::InvalidGraph`] wrapping the
//! [`CorpusError`](super::CorpusError) — preserved end to end. The
//! function is pure: no I/O, no environment reads, no statics.

use std::fmt;

use thiserror::Error;

use super::graph::{CorpusGraph, EdgeKind, Node, TraceMetadata};
use super::lifecycle::{LifecycleError, RequirementLifecycle, evaluate_all_lifecycles};

/// Explicit lifecycle-enforcement input (LLR-119).
///
/// The caller names its policy; there is no default and no DAL
/// inference. Exactly one policy exists — enforcement is required —
/// so constructing this value is a deliberate act, and exhaustive
/// matches pin the variant set against silent weakening.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleEnforcement {
    /// Require every gated claim to target an
    /// [`RequirementLifecycle::Approved`] requirement.
    Required,
}

/// The artifact whose claim against a requirement is gated
/// (LLR-119).
///
/// Declaration order is the deterministic violation sort order:
/// verifying tests first, then implementation module claims, then
/// emitted diagnostic code claims; payload fields break ties within
/// a variant.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum ReferringArtifact {
    /// A test's [`EdgeKind::Verifies`] edge targets the requirement.
    Test {
        /// Uid of the referring test.
        test_uid: String,
        /// Human-readable identifier of the referring test.
        test_id: String,
    },
    /// The requirement claims implementation modules.
    ImplementationModules {
        /// The claimed modules, in graph-canonical order.
        modules: Vec<String>,
    },
    /// The requirement claims emitted implementation diagnostic
    /// codes.
    EmittedDiagnostics {
        /// The claimed diagnostic codes, in graph-canonical order.
        codes: Vec<String>,
    },
}

/// One non-success diagnostic: a referring artifact claims an
/// unapproved requirement (LLR-119).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalBoundaryViolation {
    /// Uid of the claimed requirement.
    pub requirement_uid: String,
    /// Human-readable identifier of the claimed requirement.
    pub requirement_id: String,
    /// The requirement's evaluated lifecycle state — always
    /// `Candidate`, `Rejected`, or `Stale`, never `Approved`.
    pub state: RequirementLifecycle,
    /// The artifact carrying the gated claim.
    pub referring: ReferringArtifact,
}

impl fmt::Display for ApprovalBoundaryViolation {
    /// One line naming the requirement uid, human id, lifecycle
    /// state, and the referring artifact.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "requirement {} ({}) is {}",
            self.requirement_uid,
            self.requirement_id,
            self.state.as_str()
        )?;
        match &self.referring {
            ReferringArtifact::Test { test_uid, test_id } => {
                write!(f, "; claimed by verifying test {test_uid} ({test_id})")
            }
            ReferringArtifact::ImplementationModules { modules } => {
                write!(f, "; claims implementation modules {}", modules.join(", "))
            }
            ReferringArtifact::EmittedDiagnostics { codes } => {
                write!(f, "; claims emitted diagnostic codes {}", codes.join(", "))
            }
        }
    }
}

/// Errors from approval-boundary validation (LLR-119).
///
/// Flat variants in the [`CorpusError`](super::CorpusError) idiom:
/// every failure fails closed with the context needed to fix it.
#[derive(Debug, Error)]
pub enum ApprovalBoundaryError {
    /// At least one gated claim targets an unapproved requirement.
    /// Violations are sorted by requirement uid, then referring
    /// artifact.
    #[error(
        "explicit lifecycle enforcement rejected {} gated claim(s):\n{}",
        violations.len(),
        render_violations(violations)
    )]
    Violations {
        /// Every violating claim, aggregated (never first-fail).
        violations: Vec<ApprovalBoundaryViolation>,
    },
    /// Lifecycle evaluation failed because the graph is malformed.
    /// Impossible-invariant failures — dangling edges, review-graph
    /// invariant violations, evaluation failures — are carried with
    /// their full source chain ([`LifecycleError::InvalidGraph`]
    /// wrapping the typed [`CorpusError`](super::CorpusError), which
    /// may itself wrap a [`ReviewError`](super::ReviewError)): they
    /// are never flattened into
    /// [`ApprovalBoundaryError::Violations`] and never silently
    /// skipped.
    #[error("lifecycle evaluation failed under explicit enforcement: {0}")]
    Lifecycle(#[from] LifecycleError),
    /// A requirement node has no entry in the lifecycle evaluation
    /// map. `evaluate_all_lifecycles` covers every requirement node
    /// in the graph, so a missing entry means the evaluator and the
    /// graph disagree — an impossible invariant, returned as a typed
    /// error instead of silently skipping the requirement's claims.
    /// Unreachable through the public entry points today; it exists
    /// so a future evaluator change degrades loudly.
    #[error(
        "requirement {requirement_uid} has no lifecycle evaluation; \
         the evaluator covers every requirement node"
    )]
    InvariantMissingEvaluation {
        /// The requirement uid missing from the evaluation map.
        requirement_uid: String,
    },
}

/// Validate that no implementation or verification evidence claims
/// an unapproved requirement (LLR-120, LLR-121).
///
/// See the module documentation for the full contract: the gated
/// claim set, the zero-review fail-closed rule, the compatibility
/// boundary, and the deterministic reporting order.
///
/// # Errors
///
/// - [`ApprovalBoundaryError::Lifecycle`] when the graph fails
///   validation inside lifecycle evaluation — the typed source
///   chain is preserved, never flattened.
/// - [`ApprovalBoundaryError::Violations`] aggregating every gated
///   claim whose target is not [`RequirementLifecycle::Approved`].
/// - [`ApprovalBoundaryError::InvariantMissingEvaluation`] when a
///   requirement node is missing from the evaluation map — an
///   impossible invariant, returned instead of silently skipping.
pub fn validate_approval_boundary(
    graph: &CorpusGraph,
    enforcement: LifecycleEnforcement,
) -> Result<(), ApprovalBoundaryError> {
    match enforcement {
        LifecycleEnforcement::Required => validate_required(graph),
    }
}

/// The one policy: every gated claim must target an approved
/// requirement.
fn validate_required(graph: &CorpusGraph) -> Result<(), ApprovalBoundaryError> {
    let evaluations = evaluate_all_lifecycles(graph)?;
    let mut violations = Vec::new();
    for node in graph.nodes() {
        match node {
            Node::Test(test) => {
                for (kind, target) in &test.edges {
                    if *kind != EdgeKind::Verifies {
                        continue;
                    }
                    let (Some(target_node), Some(evaluation)) =
                        (graph.get(target), evaluations.get(target))
                    else {
                        // Unreachable through this entry point:
                        // `evaluate_all_lifecycles` above runs
                        // `CorpusGraph::validate` first, which
                        // rejects a dangling or non-requirement
                        // `Verifies` target before this scan runs.
                        // The branch remains as defense in depth, so
                        // even an unvalidated graph fails closed here
                        // rather than passing silently.
                        return Err(LifecycleError::RequirementMissing {
                            requirement_uid: target.clone(),
                        }
                        .into());
                    };
                    if evaluation.state == RequirementLifecycle::Approved {
                        continue;
                    }
                    violations.push(ApprovalBoundaryViolation {
                        requirement_uid: target.clone(),
                        requirement_id: target_node.id().to_string(),
                        state: evaluation.state,
                        referring: ReferringArtifact::Test {
                            test_uid: test.uid.clone(),
                            test_id: test.id.clone(),
                        },
                    });
                }
            }
            Node::Requirement(requirement) => {
                let Some(evaluation) = evaluations.get(&requirement.uid) else {
                    // `evaluate_all_lifecycles` covers every
                    // requirement node, so a missing evaluation is an
                    // impossible invariant. Fail closed with a typed
                    // error — a requirement's claims are never
                    // skipped silently.
                    return Err(ApprovalBoundaryError::InvariantMissingEvaluation {
                        requirement_uid: requirement.uid.clone(),
                    });
                };
                if evaluation.state == RequirementLifecycle::Approved {
                    continue;
                }
                let Some(TraceMetadata::Requirement(metadata)) =
                    graph.trace_metadata(&requirement.uid)
                else {
                    continue;
                };
                if !metadata.modules.is_empty() {
                    violations.push(ApprovalBoundaryViolation {
                        requirement_uid: requirement.uid.clone(),
                        requirement_id: requirement.id.clone(),
                        state: evaluation.state,
                        referring: ReferringArtifact::ImplementationModules {
                            modules: metadata.modules.clone(),
                        },
                    });
                }
                if !metadata.emits.is_empty() {
                    violations.push(ApprovalBoundaryViolation {
                        requirement_uid: requirement.uid.clone(),
                        requirement_id: requirement.id.clone(),
                        state: evaluation.state,
                        referring: ReferringArtifact::EmittedDiagnostics {
                            codes: metadata.emits.clone(),
                        },
                    });
                }
            }
            Node::Review(_) => {}
            // Source revisions make no implementation or
            // verification claims.
            Node::SourceRevision(_) => {}
        }
    }
    violations.sort_by(|left, right| {
        (&left.requirement_uid, &left.referring).cmp(&(&right.requirement_uid, &right.referring))
    });
    if violations.is_empty() {
        Ok(())
    } else {
        Err(ApprovalBoundaryError::Violations { violations })
    }
}

/// The `Violations` `Display` body: one line per violation, already
/// sorted by the caller.
fn render_violations(violations: &[ApprovalBoundaryViolation]) -> String {
    violations
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

// Unit tests and their shared fixtures live in sibling files under
// `approval_boundary/`, pulled in via `#[path]`.
#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
#[path = "approval_boundary/fixtures.rs"]
mod fixtures;
#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
#[path = "approval_boundary/tests.rs"]
mod tests;
