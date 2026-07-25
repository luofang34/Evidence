//! The re-ingestion drift comparison entry point (LLR-176).
//!
//! [`compare_reingestion`] is a pure, read-only core API: no I/O,
//! no fetch, no environment reads, no mutable state, and no writes
//! of any kind. It never mints committed identities, never applies
//! unapproved patches, never mutates a graph, never rewrites
//! source files, and never updates `sources.lock`; every input is
//! byte-identical afterwards. CLI/MCP presentation and any
//! human-authorized baseline write are the surface milestone's
//! concern, not this module's.
//!
//! # Prerequisite validation
//!
//! Comparison refuses — before any finding is computed — when any
//! prerequisite is invalid, in this documented order so the
//! reported error is deterministic:
//!
//! 1. **Committed baseline** — the corpus graph must pass
//!    [`CorpusGraph::validate`].
//! 2. **Source revision** — the named revision must exist; its
//!    media type binds the candidate validation.
//! 3. **Candidate revision binding** — every candidate node must
//!    bind the compared revision.
//! 4. **Candidate parser graph** — the standalone source-graph
//!    validator must pass.
//! 5. **Review evaluations** — every committed and candidate patch
//!    bound to the revision must carry an evaluation on its plane.
//!
//! Per-item malformation inside otherwise valid planes degrades to
//! findings instead ([`DriftCategory::PatchUnappliable`]), so one
//! bad patch never hides later independent findings.
//!
//! # Planes
//!
//! Recipe, verified input, extractor output, parser output, patch,
//! review, and effective output compare as separate identities: a
//! targeted mutation of one plane moves exactly that plane's
//! findings. The extractor-output plane (PDF only) compares only
//! when the baseline carries an extractor-output digest; a baseline
//! without one (a non-PDF revision) skips the plane entirely, so
//! `None` on the candidate side is drift only against a present
//! baseline identity. Findings sort by category, structural path,
//! and uid under the report's source document and revision;
//! identical planes return explicit equality with zero findings.

use std::collections::BTreeMap;

use super::super::digest::StructuralContentDigest;
use super::super::effective_graph::effective_source_graph;
use super::super::graph::{CorpusGraph, Node};
use super::super::ingest::recipe::IngesterRecipe;
use super::super::patch_lifecycle::PatchLifecycleEvaluation;
use super::super::source_graph::SourceGraph;
use super::super::source_graph::validate::validate_graph_standalone;
use super::super::source_patch::apply::PatchBindings;
use super::super::source_patch::digest::source_graph_digest;
use super::super::source_patch::records::SourcePatchRecord;
use super::error::DriftError;
use super::findings::{DriftCategory, DriftDetail, DriftFinding, DriftReport};
use super::{nodes, patches};

/// The committed side of one re-ingestion comparison (LLR-176):
/// the validated corpus graph, the compared revision, and the
/// baseline identity digests the committed parser graph was
/// produced under.
#[derive(Debug)]
pub struct DriftBaseline<'a> {
    /// The committed corpus graph; validated inside.
    pub corpus: &'a CorpusGraph,
    /// The `src_<UUIDv4>` revision being re-ingested.
    pub source_revision_uid: &'a str,
    /// The baseline recipe identity plane: the digest of the
    /// ingester recipe the committed parser graph was produced
    /// under.
    pub recipe_digest: StructuralContentDigest,
    /// The baseline verified-input identity plane.
    pub input_digest: StructuralContentDigest,
    /// The baseline extractor-output identity plane (PDF only;
    /// LLR-183); `None` for non-PDF revisions, which skips the
    /// plane.
    pub extractor_output_digest: Option<StructuralContentDigest>,
    /// The committed plane's patch lifecycle evaluations (from
    /// [`evaluate_all_patch_lifecycles`]).
    ///
    /// [`evaluate_all_patch_lifecycles`]: super::super::patch_lifecycle::evaluate_all_patch_lifecycles
    pub patch_evaluations: &'a BTreeMap<String, PatchLifecycleEvaluation>,
}

/// The candidate side of one re-ingestion comparison (LLR-176):
/// what the re-ingestion actually presented. An absent recipe or
/// input identity is drift (`recipe changed or unavailable`,
/// `verified input changed, missing, or unverifiable`), not a
/// refusal.
#[derive(Debug)]
pub struct ReingestionCandidate<'a> {
    /// The source document's canonical identity (path or URL) as
    /// presented by the re-ingestion; the report's first sort key.
    pub source_document: &'a str,
    /// The exact ingester recipe identity, when available.
    pub recipe: Option<&'a IngesterRecipe>,
    /// The verified digest of the re-ingested bytes; `None` when
    /// the input was missing or unverifiable upstream.
    pub verified_input_digest: Option<StructuralContentDigest>,
    /// The candidate extractor-output identity plane (LLR-183);
    /// `None` when the re-ingestion produced no extractor-output
    /// identity.
    pub extractor_output_digest: Option<StructuralContentDigest>,
    /// The candidate parser graph; validated standalone inside.
    pub parser_graph: &'a SourceGraph,
    /// The candidate plane's applicable curated patches.
    pub patches: &'a [SourcePatchRecord],
    /// The candidate plane's patch lifecycle evaluations.
    pub patch_evaluations: &'a BTreeMap<String, PatchLifecycleEvaluation>,
}

/// Compare the candidate planes of one re-ingestion against the
/// committed baseline (LLR-176..LLR-178). Pure and read-only; see
/// the module docs for the prerequisite order and the plane
/// semantics.
///
/// # Errors
///
/// Fails before comparison — with no findings computed — on an
/// invalid committed baseline, an unknown source revision, a
/// candidate node bound to another revision, an invalid candidate
/// parser graph, an incomplete evaluation plane, or a committed
/// effective graph that cannot be produced.
pub fn compare_reingestion(
    baseline: &DriftBaseline,
    candidate: &ReingestionCandidate,
) -> Result<DriftReport, DriftError> {
    baseline
        .corpus
        .validate()
        .map_err(|source| DriftError::InvalidBaseline(Box::new(source)))?;
    let revision_uid = baseline.source_revision_uid;
    let media_type = revision_media_type(baseline.corpus, revision_uid)?;
    for node in candidate.parser_graph.nodes() {
        if node.source_revision_uid != revision_uid {
            return Err(DriftError::CandidateRevisionMismatch {
                revision_uid: revision_uid.to_string(),
                node_uid: node.uid.clone(),
                found: node.source_revision_uid.clone(),
            });
        }
    }
    validate_graph_standalone(revision_uid, &media_type, candidate.parser_graph)
        .map_err(|source| DriftError::InvalidCandidateGraph(Box::new(source)))?;
    let committed_patches: BTreeMap<String, SourcePatchRecord> = baseline
        .corpus
        .source_patches()
        .iter()
        .filter(|(_, patch)| patch.source_revision_uid == revision_uid)
        .map(|(uid, patch)| (uid.clone(), patch.clone()))
        .collect();
    for uid in committed_patches.keys() {
        if !baseline.patch_evaluations.contains_key(uid) {
            return Err(DriftError::MissingPatchEvaluation {
                plane: "committed",
                patch_uid: uid.clone(),
            });
        }
    }
    let candidate_patches: Vec<SourcePatchRecord> = candidate
        .patches
        .iter()
        .filter(|patch| patch.source_revision_uid == revision_uid)
        .cloned()
        .collect();
    for patch in &candidate_patches {
        if !candidate.patch_evaluations.contains_key(&patch.uid) {
            return Err(DriftError::MissingPatchEvaluation {
                plane: "candidate",
                patch_uid: patch.uid.clone(),
            });
        }
    }

    let mut findings = Vec::new();
    let candidate_recipe_digest = candidate.recipe.map(IngesterRecipe::digest);
    if candidate_recipe_digest.as_ref() != Some(&baseline.recipe_digest) {
        findings.push(plane_finding(
            DriftCategory::RecipeChangedOrUnavailable,
            &baseline.recipe_digest,
            candidate_recipe_digest.as_ref(),
        ));
    }
    if candidate.verified_input_digest.as_ref() != Some(&baseline.input_digest) {
        findings.push(plane_finding(
            DriftCategory::VerifiedInputChanged,
            &baseline.input_digest,
            candidate.verified_input_digest.as_ref(),
        ));
    }
    if let Some(baseline_extractor) = &baseline.extractor_output_digest
        && candidate.extractor_output_digest.as_ref() != Some(baseline_extractor)
    {
        findings.push(plane_finding(
            DriftCategory::ExtractorOutputChanged,
            baseline_extractor,
            candidate.extractor_output_digest.as_ref(),
        ));
    }

    let empty = SourceGraph::new();
    let committed_graph = baseline.corpus.source_graph(revision_uid).unwrap_or(&empty);
    findings.extend(nodes::compare_nodes(
        committed_graph,
        candidate.parser_graph,
    ));

    let outcome = patches::compare_patches(
        &media_type,
        &committed_patches,
        baseline.patch_evaluations,
        &patches::CandidatePatchPlane {
            graph: candidate.parser_graph,
            patches: &candidate_patches,
            evaluations: candidate.patch_evaluations,
            recipe_digest: candidate_recipe_digest.as_ref(),
            input_digest: candidate.verified_input_digest.as_ref(),
        },
    );
    findings.extend(outcome.findings);

    let bindings = PatchBindings {
        recipe_digest: baseline.recipe_digest.clone(),
        input_digest: baseline.input_digest.clone(),
    };
    let committed_effective =
        effective_source_graph(baseline.corpus, revision_uid, &bindings, &media_type)
            .map_err(|source| DriftError::InvalidEffectiveBaseline(Box::new(source)))?;
    let committed_digest = source_graph_digest(&committed_effective.graph);
    let candidate_digest = source_graph_digest(&outcome.effective);
    if committed_digest != candidate_digest {
        findings.push(DriftFinding {
            category: DriftCategory::EffectiveGraphChanged,
            structural_path: None,
            node_uid: None,
            patch_uid: None,
            detail: DriftDetail::EffectiveDigests {
                committed: committed_digest.as_str().to_string(),
                candidate: candidate_digest.as_str().to_string(),
            },
        });
    }

    findings.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()));
    Ok(DriftReport {
        source_document: candidate.source_document.to_string(),
        source_revision_uid: revision_uid.to_string(),
        findings,
    })
}

/// The compared revision's declared media type.
fn revision_media_type(graph: &CorpusGraph, revision_uid: &str) -> Result<String, DriftError> {
    for node in graph.nodes() {
        if let Node::SourceRevision(revision) = node
            && revision.uid == revision_uid
        {
            return Ok(revision.media_type.clone());
        }
    }
    Err(DriftError::UnknownSourceRevision {
        revision_uid: revision_uid.to_string(),
    })
}

fn plane_finding(
    category: DriftCategory,
    baseline: &StructuralContentDigest,
    candidate: Option<&StructuralContentDigest>,
) -> DriftFinding {
    DriftFinding {
        category,
        structural_path: None,
        node_uid: None,
        patch_uid: None,
        detail: DriftDetail::PlaneDigest {
            baseline: baseline.as_str().to_string(),
            candidate: candidate.map(|digest| digest.as_str().to_string()),
        },
    }
}
