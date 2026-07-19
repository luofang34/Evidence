//! Graph-derived requirement-report parity and gap coverage.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use evidence_core::bundle::TestOutcome;
use evidence_core::corpus::{
    CorpusGraph, EdgeKind, Node, RequirementLayer, RequirementNode, TestNode,
    graph_from_trace_files,
};
use evidence_core::policy::TracePolicy;
use evidence_core::trace::{
    RequirementReportError, build_corpus_requirement_report, build_requirement_report,
    read_all_trace_files,
};

#[test]
fn corpus_gap_report_matches_adapter_and_ignores_input_order() {
    let root = workspace_root();
    let trace_root = root.join("cert/trace");
    let trace_path = trace_root.to_str().expect("trace path is UTF-8");
    let original = read_all_trace_files(trace_path).expect("read original trace");
    let mut reordered = read_all_trace_files(trace_path).expect("read reordered trace");
    reverse_trace_input(&mut reordered);
    let outcomes = passing_outcomes(&original);

    let original_graph = graph_from_trace_files(&original).expect("adapt original trace");
    let reordered_graph = graph_from_trace_files(&reordered).expect("adapt reordered trace");
    assert_eq!(original_graph, reordered_graph);

    let direct =
        build_corpus_requirement_report(&original_graph, &outcomes, &root, &TracePolicy::default())
            .expect("build graph report");
    let adapted = build_requirement_report(&original, &outcomes, &root, &TracePolicy::default());
    let reordered_report =
        build_requirement_report(&reordered, &outcomes, &root, &TracePolicy::default());

    assert_eq!(direct, adapted);
    assert_eq!(adapted, reordered_report);
    assert!(
        adapted
            .iter()
            .all(|diagnostic| diagnostic.code == "REQ_PASS")
    );
}

#[test]
fn corpus_gap_report_covers_structural_and_execution_gaps() {
    let root = workspace_root();
    let outcomes = BTreeMap::from([(
        "corpus_gap_reports::corpus_gap_report_covers_structural_and_execution_gaps".to_string(),
        TestOutcome::Passed,
    )]);

    assert_gap(
        fixture(FixtureGap::MissingParent),
        &outcomes,
        &root,
        "LLR LLR-1 has no parent HLR edge",
    );
    assert_gap(
        fixture(FixtureGap::MissingVerification),
        &outcomes,
        &root,
        "LLR LLR-1 has no TEST coverage",
    );
    assert_gap(
        fixture(FixtureGap::OrphanTest),
        &outcomes,
        &root,
        "TEST TEST-1 is orphaned",
    );
    assert_gap(
        fixture(FixtureGap::UnresolvedSelector),
        &outcomes,
        &root,
        "did not resolve to a real #[test] fn",
    );
    assert_gap(
        fixture(FixtureGap::WrongLayer),
        &outcomes,
        &root,
        "TEST TEST-1 verifies HLR layer; expected LLR",
    );

    let strict = TracePolicy {
        require_hlr_verification_methods: true,
        ..TracePolicy::default()
    };
    let diagnostics =
        build_corpus_requirement_report(&fixture(FixtureGap::None), &outcomes, &root, &strict)
            .expect("strict report");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "REQ_GAP"
            && diagnostic.message == "HLR HLR-1 is missing verification methods"
    }));
}

#[test]
fn corpus_gap_report_rejects_unsupported_edge_kinds() {
    let mut graph = CorpusGraph::new();
    insert_requirement(&mut graph, "h", "HLR-1", RequirementLayer::Hlr, vec![]);
    graph
        .insert(Node::Test(TestNode {
            uid: "t".into(),
            id: "TEST-1".into(),
            title: "test".into(),
            selectors: vec!["corpus_gap_report_rejects_unsupported_edge_kinds".into()],
            edges: vec![(EdgeKind::DerivesFrom, "h".into())],
        }))
        .expect("insert test");

    let error = build_corpus_requirement_report(
        &graph,
        &BTreeMap::new(),
        &workspace_root(),
        &TracePolicy::default(),
    )
    .expect_err("unsupported edge must fail closed");
    assert!(matches!(
        error,
        RequirementReportError::UnsupportedEdge {
            from,
            kind: EdgeKind::DerivesFrom,
        } if from == "t"
    ));
}

#[derive(Clone, Copy)]
enum FixtureGap {
    None,
    MissingParent,
    MissingVerification,
    OrphanTest,
    UnresolvedSelector,
    WrongLayer,
}

fn fixture(gap: FixtureGap) -> CorpusGraph {
    let mut graph = CorpusGraph::new();
    insert_requirement(&mut graph, "s", "SYS-1", RequirementLayer::Sys, vec![]);
    insert_requirement(
        &mut graph,
        "h",
        "HLR-1",
        RequirementLayer::Hlr,
        vec![(EdgeKind::DerivesFrom, "s".into())],
    );
    let llr_edges = if matches!(gap, FixtureGap::MissingParent) {
        vec![]
    } else {
        vec![(EdgeKind::DerivesFrom, "h".into())]
    };
    insert_requirement(&mut graph, "l", "LLR-1", RequirementLayer::Llr, llr_edges);
    if !matches!(gap, FixtureGap::MissingVerification) {
        let test_edges = match gap {
            FixtureGap::OrphanTest => vec![],
            FixtureGap::WrongLayer => vec![(EdgeKind::Verifies, "h".into())],
            FixtureGap::None
            | FixtureGap::MissingParent
            | FixtureGap::MissingVerification
            | FixtureGap::UnresolvedSelector => vec![(EdgeKind::Verifies, "l".into())],
        };
        let selector = if matches!(gap, FixtureGap::UnresolvedSelector) {
            "corpus_gap_reports::selector_that_does_not_exist"
        } else {
            "corpus_gap_reports::corpus_gap_report_covers_structural_and_execution_gaps"
        };
        graph
            .insert(Node::Test(TestNode {
                uid: "t".into(),
                id: "TEST-1".into(),
                title: "test".into(),
                selectors: vec![selector.into()],
                edges: test_edges,
            }))
            .expect("insert test");
    }
    graph
}

fn insert_requirement(
    graph: &mut CorpusGraph,
    uid: &str,
    id: &str,
    layer: RequirementLayer,
    edges: Vec<(EdgeKind, String)>,
) {
    graph
        .insert(Node::Requirement(RequirementNode {
            uid: uid.into(),
            id: id.into(),
            title: id.into(),
            layer,
            edges,
        }))
        .expect("insert requirement");
}

fn assert_gap(
    graph: CorpusGraph,
    outcomes: &BTreeMap<String, TestOutcome>,
    root: &Path,
    message: &str,
) {
    let diagnostics =
        build_corpus_requirement_report(&graph, outcomes, root, &TracePolicy::default())
            .expect("build gap report");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "REQ_GAP" && diagnostic.message.contains(message)
    }));
}

fn passing_outcomes(trace: &evidence_core::trace::TraceFiles) -> BTreeMap<String, TestOutcome> {
    trace
        .tests
        .tests
        .iter()
        .flat_map(evidence_core::trace::TestEntry::all_selectors)
        .map(|selector| (selector, TestOutcome::Passed))
        .collect()
}

fn reverse_trace_input(trace: &mut evidence_core::trace::TraceFiles) {
    trace.sys.requirements.reverse();
    trace.hlr.requirements.reverse();
    trace.llr.requirements.reverse();
    trace.tests.tests.reverse();
    for requirement in trace
        .sys
        .requirements
        .iter_mut()
        .chain(trace.hlr.requirements.iter_mut())
    {
        requirement.traces_to.reverse();
        requirement.verification_methods.reverse();
    }
    for requirement in &mut trace.llr.requirements {
        requirement.traces_to.reverse();
        requirement.verification_methods.reverse();
    }
    for test in &mut trace.tests.tests {
        test.traces_to.reverse();
        test.test_selectors.reverse();
    }
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("evidence-core lives under crates")
        .to_path_buf()
}
