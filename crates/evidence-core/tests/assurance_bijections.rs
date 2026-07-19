//! Corpus-derived assurance mapping parity and failure-path tests.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use evidence_core::corpus::graph_from_trace_files;
use evidence_core::trace::{AssuranceBijections, TraceFiles, read_all_trace_files};

type Claimants = BTreeMap<String, BTreeSet<String>>;

fn own_trace_blocking() -> TraceFiles {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates directory")
        .parent()
        .expect("workspace root")
        .join("cert/trace");
    read_all_trace_files(&root.to_string_lossy()).expect("read own trace")
}

fn legacy_claimants(trace: &TraceFiles) -> (Claimants, Claimants) {
    let mut surfaces = Claimants::new();
    for requirement in &trace.hlr.requirements {
        add_claims(&mut surfaces, &requirement.id, &requirement.surfaces);
    }
    let mut diagnostics = Claimants::new();
    for requirement in &trace.llr.requirements {
        add_claims(&mut diagnostics, &requirement.id, &requirement.emits);
    }
    (surfaces, diagnostics)
}

fn add_claims(claimants: &mut Claimants, requirement_id: &str, claims: &[String]) {
    for claim in claims {
        claimants
            .entry(claim.clone())
            .or_default()
            .insert(requirement_id.to_string());
    }
}

#[test]
fn graph_assurance_mappings_match_legacy_and_ignore_input_order() {
    let trace = own_trace_blocking();
    let expected = legacy_claimants(&trace);
    let graph = graph_from_trace_files(&trace).expect("adapt own trace");
    graph.validate().expect("own graph validates");
    let mappings = AssuranceBijections::from_graph(&graph);
    assert_eq!(mappings.surface_claimants(), &expected.0);
    assert_eq!(mappings.diagnostic_claimants(), &expected.1);

    let mut reordered = own_trace_blocking();
    reordered.hlr.requirements.reverse();
    reordered.llr.requirements.reverse();
    for requirement in &mut reordered.hlr.requirements {
        requirement.surfaces.reverse();
    }
    for requirement in &mut reordered.llr.requirements {
        requirement.emits.reverse();
    }
    let reordered_graph = graph_from_trace_files(&reordered).expect("adapt reordered trace");
    assert_eq!(
        graph, reordered_graph,
        "claim order must not change the graph"
    );
    assert_eq!(
        mappings,
        AssuranceBijections::from_graph(&reordered_graph),
        "record and claim order must be non-semantic"
    );
}

#[test]
fn graph_diagnostic_bijection_reports_unknown_and_unclaimed_codes() {
    let mut trace = own_trace_blocking();
    let removed = evidence_core::RULES
        .iter()
        .map(|rule| rule.code)
        .find(|code| !evidence_core::RESERVED_UNCLAIMED_CODES.contains(code))
        .expect("at least one required diagnostic code");
    for requirement in &mut trace.llr.requirements {
        requirement.emits.retain(|code| code != removed);
    }
    trace.llr.requirements[0]
        .emits
        .push("NOT_A_RULE".to_string());

    let graph = graph_from_trace_files(&trace).expect("adapt trace fixture");
    let mappings = AssuranceBijections::from_graph(&graph);
    let known: Vec<&str> = evidence_core::RULES.iter().map(|rule| rule.code).collect();
    let diff = mappings.diagnostic_diff(&known, evidence_core::RESERVED_UNCLAIMED_CODES);

    assert_eq!(diff.unknown, vec!["NOT_A_RULE"]);
    assert!(
        diff.unclaimed.contains(&removed.to_string()),
        "removed RULES code must be reported as unclaimed"
    );
}
