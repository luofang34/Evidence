//! Tests for the pure immutable-superset source transition
//! comparison (TEST-148).

use super::fixtures::*;
use super::validate_source_transition;
use crate::corpus::{
    CorpusError, CorpusIndex, EdgeKind, Node, SourceCapture, SourceError, SourceMaterial,
};

/// An unchanged baseline transitions into itself, and a proper new
/// revision extending the prior effective head succeeds (TEST-148).
#[test]
fn proper_extension_succeeds() {
    let prior = graph_of(two_chain(DOC_1));
    assert!(
        validate_source_transition(&prior, &prior.clone()).is_ok(),
        "an unchanged baseline transitions into itself"
    );

    let proposed = graph_of(three_chain(DOC_1));
    assert!(
        validate_source_transition(&prior, &proposed).is_ok(),
        "a new revision superseding the prior head extends the chain"
    );
    assert_eq!(
        super::effective_source_heads(&proposed),
        [(DOC_1.to_string(), SRC_C.to_string())]
            .into_iter()
            .collect(),
        "the new revision becomes the effective head"
    );
}

/// Revisions of a document key the prior baseline never saw are
/// unconstrained (TEST-148).
#[test]
fn new_document_key_is_free() {
    let prior = graph_of(two_chain(DOC_1));
    let mut nodes = two_chain(DOC_1);
    nodes.extend([
        revision(SRC_D, DOC_2, None),
        revision(SRC_E, DOC_2, Some(SRC_D)),
    ]);
    let proposed = graph_of(nodes);
    assert!(
        validate_source_transition(&prior, &proposed).is_ok(),
        "a new document key with its own chain is free"
    );
}

/// Requirement additions and edits never affect the source
/// transition, which is scoped to the source-revision subgraph
/// (TEST-148).
#[test]
fn non_source_differences_are_ignored() {
    let mut prior_nodes = two_chain(DOC_1);
    prior_nodes.push(requirement(REQ_A));
    let prior = graph_of(prior_nodes);

    let mut proposed_nodes = two_chain(DOC_1);
    let mut edited = requirement(REQ_A);
    let Node::Requirement(requirement_node) = &mut edited else {
        unreachable!("requirement() builds a requirement")
    };
    requirement_node.title = "edited title".to_string();
    requirement_node.description = Some("new description".to_string());
    proposed_nodes.push(edited);
    proposed_nodes.push(requirement(REQ_B));
    let proposed = graph_of(proposed_nodes);

    assert!(
        validate_source_transition(&prior, &proposed).is_ok(),
        "non-source additions and edits are ignored"
    );
}

/// Mutating any field of a retained revision's immutable
/// projection fails with a typed mutation naming the revision and
/// the differing field (TEST-148).
#[test]
fn mutation_of_each_projection_field_fails_naming_the_field() {
    assert_mutation(SRC_B, "id", |revision| {
        revision.id = "RENAMED".to_string();
    });
    assert_mutation(SRC_B, "document_key", |revision| {
        revision.document_key = DOC_2.to_string();
    });
    assert_mutation(SRC_B, "title", |revision| {
        revision.title = "edited".to_string();
    });
    assert_mutation(SRC_B, "media_type", |revision| {
        revision.media_type = "text/plain".to_string();
    });
    assert_mutation(SRC_B, "canonical_location", |revision| {
        revision.canonical_location = "https://evil.example/pwned".to_string();
    });
    assert_mutation(SRC_B, "retrieved_at", |revision| {
        revision.material = material_at(DIGEST_A, "2026-07-02T10:00:00Z");
    });
    assert_mutation(SRC_B, "sha256", |revision| {
        revision.material = available_material(DIGEST_B);
    });
    assert_mutation(SRC_B, "capture", |revision| {
        revision.material = hash_only_material(DIGEST_A);
    });
    assert_mutation(SRC_B, "capture", |revision| {
        revision.material = SourceMaterial::Available {
            retrieved_at: "2026-07-01T10:00:00Z".to_string(),
            sha256: crate::corpus::SourceContentDigest::from_hex(DIGEST_A).unwrap(),
            capture: SourceCapture::Vendored {
                path: "sources/doc/renamed.pdf".to_string(),
            },
        };
    });
    assert_mutation(SRC_B, "material", |revision| {
        revision.material = unavailable_material("upstream gone");
    });
    assert_mutation(SRC_B, "supersedes", |revision| {
        revision.edges = vec![(EdgeKind::Supersedes, SRC_C.to_string())];
    });
    assert_mutation(SRC_B, "supersedes", |revision| {
        revision.edges = Vec::new();
    });
    assert_mutation(SRC_A, "supersedes", |revision| {
        revision.edges = vec![(EdgeKind::Supersedes, SRC_B.to_string())];
    });

    // Unavailable material: the reason is projection content too.
    let prior = graph_of(vec![revision_with_material(
        SRC_A,
        DOC_1,
        None,
        unavailable_material("upstream returned 404"),
    )]);
    let proposed = graph_of(vec![revision_with_material(
        SRC_A,
        DOC_1,
        None,
        unavailable_material("rewritten reason"),
    )]);
    let err = validate_source_transition(&prior, &proposed).unwrap_err();
    assert!(
        matches!(
            err,
            SourceError::SourceTransitionMutation { ref uid, field }
                if uid == SRC_A && field == "reason"
        ),
        "a changed reason must fail with a typed mutation, got: {err:?}"
    );

    // A different node kind at a retained uid is an identity
    // mutation, not an update.
    let proposed = graph_of(vec![
        requirement(SRC_A),
        revision(SRC_B, DOC_1, Some(SRC_A)),
    ]);
    let err = validate_source_transition(&graph_of(two_chain(DOC_1)), &proposed).unwrap_err();
    assert!(
        matches!(
            err,
            SourceError::SourceTransitionMutation { ref uid, field }
                if uid == SRC_A && field == "kind"
        ),
        "a kind change at a retained uid must fail with a typed mutation, got: {err:?}"
    );
}

/// Apply `mutate` to one revision of the two-revision prior chain
/// and assert the transition fails with a mutation naming the uid
/// and field.
fn assert_mutation(
    uid: &str,
    field: &'static str,
    mutate: impl FnOnce(&mut crate::corpus::SourceRevisionNode),
) {
    let prior = graph_of(two_chain(DOC_1));
    let mut proposed_nodes = two_chain(DOC_1);
    let target = proposed_nodes
        .iter_mut()
        .find(|node| node.uid() == uid)
        .unwrap();
    mutate(source_node_mut(target));
    let proposed = graph_of(proposed_nodes);
    let err = validate_source_transition(&prior, &proposed).unwrap_err();
    assert!(
        matches!(
            err,
            SourceError::SourceTransitionMutation { uid: ref found, field: found_field }
                if found == uid && found_field == field
        ),
        "mutating {field} on {uid} must fail with a typed mutation naming both, got: {err:?}"
    );
}

/// A source-revision node with explicit material, for
/// material-state fixtures.
fn revision_with_material(
    uid: &str,
    document_key: &str,
    supersedes: Option<&str>,
    material: SourceMaterial,
) -> Node {
    let mut node = revision(uid, document_key, supersedes);
    source_node_mut(&mut node).material = material;
    node
}

/// A prior revision absent from the proposed graph is a typed
/// removal — revisions are never silently dropped (TEST-148).
#[test]
fn silent_removal_fails_closed() {
    let prior = graph_of(two_chain(DOC_1));
    let proposed = graph_of(vec![revision(SRC_B, DOC_1, Some(SRC_A))]);
    let err = validate_source_transition(&prior, &proposed).unwrap_err();
    assert!(
        matches!(
            err,
            SourceError::SourceTransitionRemoval { ref uid } if uid == SRC_A
        ),
        "dropping a chained revision must fail with SourceTransitionRemoval, got: {err:?}"
    );

    let proposed = graph_of(vec![revision(SRC_A, DOC_1, None)]);
    let err = validate_source_transition(&prior, &proposed).unwrap_err();
    assert!(
        matches!(
            err,
            SourceError::SourceTransitionRemoval { ref uid } if uid == SRC_B
        ),
        "dropping the head revision must fail with SourceTransitionRemoval, got: {err:?}"
    );
}

/// Replacing the bytes at the same uid — a new digest where a
/// revision already exists — is a typed mutation, so new bytes
/// require a new `src_` uid and a supersedes edge (TEST-148).
#[test]
fn digest_replacement_at_the_same_uid_fails_closed() {
    let prior = graph_of(two_chain(DOC_1));
    let proposed = graph_of(
        two_chain(DOC_1)
            .into_iter()
            .map(|node| {
                let mut node = node;
                if node.uid() == SRC_B {
                    source_node_mut(&mut node).material = available_material(DIGEST_B);
                }
                node
            })
            .collect(),
    );
    let err = validate_source_transition(&prior, &proposed).unwrap_err();
    assert!(
        matches!(
            err,
            SourceError::SourceTransitionMutation { ref uid, field }
                if uid == SRC_B && field == "sha256"
        ),
        "a replaced digest must fail with a typed sha256 mutation, got: {err:?}"
    );
}

/// A new revision of an existing document key that does not
/// supersede the prior effective head is a typed competing head —
/// whether it carries no link at all or links to a revision that
/// is not the head (TEST-148).
#[test]
fn unlinked_competing_revision_fails_closed() {
    let prior = graph_of(vec![revision(SRC_A, DOC_1, None)]);
    let proposed = graph_of(vec![
        revision(SRC_A, DOC_1, None),
        revision(SRC_C, DOC_1, None),
    ]);
    let err = validate_source_transition(&prior, &proposed).unwrap_err();
    assert!(
        matches!(
            err,
            SourceError::SourceTransitionCompetingHead {
                ref uid,
                ref document_key,
                ref prior_head_uid,
            } if uid == SRC_C && document_key == DOC_1 && prior_head_uid == SRC_A
        ),
        "an unlinked new revision must fail with SourceTransitionCompetingHead, got: {err:?}"
    );

    let prior = graph_of(two_chain(DOC_1));
    let proposed = graph_of(vec![
        revision(SRC_A, DOC_1, None),
        revision(SRC_B, DOC_1, Some(SRC_A)),
        revision(SRC_C, DOC_1, Some(SRC_A)),
    ]);
    let err = validate_source_transition(&prior, &proposed).unwrap_err();
    assert!(
        matches!(
            err,
            SourceError::SourceTransitionCompetingHead {
                ref uid,
                ref document_key,
                ref prior_head_uid,
            } if uid == SRC_C && document_key == DOC_1 && prior_head_uid == SRC_B
        ),
        "a new revision superseding a non-head revision must fail with \
         SourceTransitionCompetingHead naming the prior head, got: {err:?}"
    );
}

/// Inserting a new revision mid-chain retargets an existing
/// revision's owned supersedes edge — a typed mutation of that
/// edge, never an edit in place (TEST-148).
#[test]
fn mid_chain_insertion_is_a_supersedes_mutation() {
    let prior = graph_of(two_chain(DOC_1));
    let proposed = graph_of(vec![
        revision(SRC_A, DOC_1, None),
        revision(SRC_C, DOC_1, Some(SRC_A)),
        revision(SRC_B, DOC_1, Some(SRC_C)),
    ]);
    let err = validate_source_transition(&prior, &proposed).unwrap_err();
    assert!(
        matches!(
            err,
            SourceError::SourceTransitionMutation { ref uid, field }
                if uid == SRC_B && field == "supersedes"
        ),
        "a retargeted supersedes edge must fail with a typed mutation, got: {err:?}"
    );
}

/// The prior graph is the trusted baseline: it must itself
/// validate, and the failure names the prior graph and carries the
/// validation error as its typed source (TEST-148).
#[test]
fn prior_graph_must_validate() {
    let prior = graph_of(vec![
        revision(SRC_A, DOC_1, None),
        revision(SRC_B, DOC_1, Some(SRC_A)),
        revision(SRC_C, DOC_1, Some(SRC_A)),
    ]);
    let proposed = graph_of(two_chain(DOC_1));
    let err = validate_source_transition(&prior, &proposed).unwrap_err();
    assert!(
        matches!(
            err,
            SourceError::SourceTransitionInvalidGraph { graph: "prior", ref source }
                if matches!(
                    source.as_ref(),
                    CorpusError::Source(SourceError::SourceSupersessionFork { .. })
                )
        ),
        "an invalid prior graph must fail with SourceTransitionInvalidGraph, got: {err:?}"
    );
}

/// A structurally invalid proposed graph fails closed even when
/// every new revision links to the prior head; the failure names
/// the proposed graph (TEST-148).
#[test]
fn proposed_graph_must_validate() {
    let prior = graph_of(vec![revision(SRC_A, DOC_1, None)]);
    let proposed = graph_of(vec![
        revision(SRC_A, DOC_1, None),
        revision(SRC_B, DOC_1, Some(SRC_A)),
        revision(SRC_C, DOC_1, Some(SRC_A)),
    ]);
    let err = validate_source_transition(&prior, &proposed).unwrap_err();
    assert!(
        matches!(
            err,
            SourceError::SourceTransitionInvalidGraph { graph: "proposed", ref source }
                if matches!(
                    source.as_ref(),
                    CorpusError::Source(SourceError::SourceSupersessionFork { .. })
                )
        ),
        "an invalid proposed graph must fail with SourceTransitionInvalidGraph, got: {err:?}"
    );
}

/// End to end through `CorpusIndex::load_graph`: a second corpus
/// adding a linked revision transitions cleanly, while a corpus
/// editing a record in place fails as a typed mutation (TEST-148).
#[test]
fn linked_revision_loads_and_transitions_through_corpus_index() {
    use super::super::tests_support::{source_file, vendored, write};

    let prior = vendored(SRC_A, "SRC-A");
    let mut middle = vendored(SRC_B, "SRC-B");
    middle.supersedes = Some(SRC_A.to_string());
    let mut newest = vendored(SRC_C, "SRC-C");
    newest.supersedes = Some(SRC_B.to_string());

    let corpus = tempfile::tempdir().unwrap();
    write(
        &corpus.path().join("corpus.toml"),
        "schema_version = 1\nsources = [\"sources/**/*.toml\"]\n",
    );
    write(
        &corpus.path().join("sources/records.toml"),
        &source_file(&[prior.clone(), middle.clone()]),
    );
    let prior_graph = CorpusIndex::load_graph(&corpus.path().join("corpus.toml")).unwrap();

    write(
        &corpus.path().join("sources/next.toml"),
        &source_file(&[newest.clone()]),
    );
    let proposed_graph = CorpusIndex::load_graph(&corpus.path().join("corpus.toml")).unwrap();
    assert!(
        validate_source_transition(&prior_graph, &proposed_graph).is_ok(),
        "a linked revision added by a second corpus file transitions"
    );
    assert_eq!(
        super::effective_source_heads(&proposed_graph),
        [("DOC-1".to_string(), SRC_C.to_string())]
            .into_iter()
            .collect(),
        "the linked revision becomes the effective head"
    );

    // The same new file, but the prior head's record is edited in
    // place — new digest at the same uid — fails as a mutation.
    let mut edited_middle = vendored(SRC_B, "SRC-B");
    edited_middle.supersedes = Some(SRC_A.to_string());
    edited_middle.material_toml = edited_middle
        .material_toml
        .replace(&"a".repeat(64), &"b".repeat(64));
    let corpus = tempfile::tempdir().unwrap();
    write(
        &corpus.path().join("corpus.toml"),
        "schema_version = 1\nsources = [\"sources/**/*.toml\"]\n",
    );
    write(
        &corpus.path().join("sources/records.toml"),
        &source_file(&[prior, edited_middle]),
    );
    write(
        &corpus.path().join("sources/next.toml"),
        &source_file(&[newest]),
    );
    let edited_graph = CorpusIndex::load_graph(&corpus.path().join("corpus.toml")).unwrap();
    let err = validate_source_transition(&prior_graph, &edited_graph).unwrap_err();
    assert!(
        matches!(
            err,
            SourceError::SourceTransitionMutation { ref uid, field }
                if uid == SRC_B && field == "sha256"
        ),
        "an in-place record edit must fail with a typed mutation, got: {err:?}"
    );
}
