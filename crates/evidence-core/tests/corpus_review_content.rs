//! Review-content digest: golden byte-lock, native≡legacy
//! projection parity, and excluded-change stability (TEST-132).
//!
//! The fixture root under `fixtures/corpus/review_content/` holds a
//! native record corpus (`native/`) and an equivalent legacy
//! four-file trace (`legacy/`) whose excluded fields (ns, owner,
//! sort_key, surfaces, modules, emits) deliberately differ. The
//! committed `review_content_v1.golden` byte-locks the v1 canonical
//! encoding and digest of the HLR fixture projection: line 1 is
//! `hex(canonical_bytes_v1(..))`, line 2 the lowercase hex SHA-256
//! digest. Regenerate with `EVIDENCE_UPDATE_FIXTURES=1`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use std::fs;
use std::path::{Path, PathBuf};

use evidence_core::corpus::{
    CorpusGraph, CorpusIndex, RequirementLayer, canonical_bytes_v1, graph_from_trace_files,
    review_content_digest_v1,
};
use evidence_core::trace::{TraceFiles, read_all_trace_files};

const SYS_UID: &str = "req_00000000-0000-4000-8000-00000000000a";
const HLR_UID: &str = "req_00000000-0000-4000-8000-00000000000b";
const LLR_UID: &str = "req_00000000-0000-4000-8000-00000000000c";

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/corpus/review_content")
}

fn native_graph() -> CorpusGraph {
    CorpusIndex::load_graph(&fixture_dir().join("native/corpus.toml"))
        .expect("load native fixture corpus")
}

fn legacy_files() -> TraceFiles {
    read_all_trace_files(&fixture_dir().join("legacy").to_string_lossy())
        .expect("read legacy fixture trace")
}

fn legacy_graph() -> CorpusGraph {
    graph_from_trace_files(&legacy_files()).expect("adapt legacy fixture trace")
}

#[test]
fn golden_fixture_byte_locks_canonical_encoding_and_digest() {
    let content = native_graph()
        .review_content(HLR_UID)
        .expect("fixture HLR projects review content");
    let digest = review_content_digest_v1(&content);
    let rendered = format!("{}\n{digest}\n", hex::encode(canonical_bytes_v1(&content)));
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/corpus/review_content_v1.golden");

    if std::env::var_os("EVIDENCE_UPDATE_FIXTURES").is_some() {
        fs::write(&path, &rendered).expect("write fixture");
        eprintln!("updated fixture: {}", path.display());
        return;
    }

    let committed = fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "missing or unreadable fixture {}: {e}\n\
             hint: run with EVIDENCE_UPDATE_FIXTURES=1 to write it",
            path.display()
        )
    });
    assert_eq!(
        committed, rendered,
        "v1 canonical encoding or digest drifted — the encoding contract is byte-locked; \
         an intentional change requires a new projection version"
    );

    // The equivalent legacy record rebuilds the same bytes.
    let legacy = legacy_graph()
        .review_content(HLR_UID)
        .expect("legacy fixture HLR projects review content");
    assert_eq!(
        review_content_digest_v1(&legacy),
        digest,
        "native and legacy fixture records must digest identically"
    );
}

#[test]
fn native_and_legacy_projections_match_for_equivalent_records() {
    let native = native_graph();
    let legacy = legacy_graph();
    for uid in [SYS_UID, HLR_UID, LLR_UID] {
        let from_native = native
            .review_content(uid)
            .expect("native record projects review content");
        let from_legacy = legacy
            .review_content(uid)
            .expect("legacy entry projects review content");
        assert_eq!(
            from_native, from_legacy,
            "native and legacy projections diverge for {uid}"
        );
        assert_eq!(
            review_content_digest_v1(&from_native),
            review_content_digest_v1(&from_legacy),
            "native and legacy digests diverge for {uid}"
        );
    }

    let hlr = native.review_content(HLR_UID).expect("native HLR projects");
    assert_eq!(hlr.title, "Review content projects from the corpus graph");
    assert_eq!(hlr.layer, RequirementLayer::Hlr);
    assert_eq!(
        hlr.description.as_deref(),
        Some("Native and legacy records expose the same projection.")
    );
    assert_eq!(
        hlr.rationale.as_deref(),
        Some("Equivalent records must approve identically.")
    );
    assert_eq!(hlr.scope.as_deref(), Some("component"));
    assert_eq!(hlr.category.as_deref(), Some("functional"));
    assert_eq!(hlr.source.as_deref(), Some("SYS-REVIEW-1"));
    assert_eq!(hlr.verification_methods, vec!["test".to_string()]);
    assert_eq!(hlr.derives_from, vec![SYS_UID.to_string()]);

    let sys = native.review_content(SYS_UID).expect("native SYS projects");
    assert_eq!(
        sys.verification_methods,
        vec!["review".to_string(), "test".to_string()],
        "unsorted file order canonicalizes on both loaders"
    );
    let llr = native.review_content(LLR_UID).expect("native LLR projects");
    assert_eq!(
        llr.rationale, None,
        "absent optional fields stay None on both loaders"
    );
}

/// Identity, presentation, and implementation-mapping changes are
/// not normative content: none of them may move the digest
/// (LLR-111, LLR-113).
#[test]
fn excluded_changes_leave_the_digest_unchanged() {
    type Variant = (&'static str, fn(&mut TraceFiles));
    let variants: [Variant; 7] = [
        ("human id rename", |files| {
            files.hlr.requirements[0].id = "HLR-RENAMED".to_string();
        }),
        ("namespace rename", |files| {
            files.hlr.requirements[0].ns = Some("OTHER".to_string());
        }),
        ("owner change", |files| {
            files.llr.requirements[0].owner = Some("other-team".to_string());
        }),
        ("sort_key change", |files| {
            files.llr.requirements[0].sort_key = Some(99);
        }),
        ("implementation modules added", |files| {
            files.llr.requirements[0]
                .modules
                .push("evidence_core::corpus::digest".to_string());
        }),
        ("surfaces and emits added", |files| {
            files.hlr.requirements[0]
                .surfaces
                .push("floors".to_string());
            files.llr.requirements[0]
                .emits
                .push("TRACE_PARSE_FAILED".to_string());
        }),
        ("record and set-like field reorder", |files| {
            files.sys.requirements.reverse();
            files.hlr.requirements.reverse();
            files.llr.requirements.reverse();
            for entry in files.llr.requirements.iter_mut() {
                entry.traces_to.reverse();
                entry.verification_methods.reverse();
            }
        }),
    ];
    for (name, mutate) in variants {
        let base = legacy_graph();
        let mut files = legacy_files();
        mutate(&mut files);
        let changed = graph_from_trace_files(&files).expect("adapt variant");
        for uid in [SYS_UID, HLR_UID, LLR_UID] {
            assert_eq!(
                review_content_digest_v1(&changed.review_content(uid).expect("variant projects")),
                review_content_digest_v1(&base.review_content(uid).expect("base projects")),
                "{name} must not change the digest of {uid}"
            );
        }
    }

    // Test mappings accrue: an added Verifies edge on the LLR leaves
    // its digest alone.
    let mut files = legacy_files();
    let mut extra_test = files.tests.tests[0].clone();
    extra_test.uid = Some("req_00000000-0000-4000-8000-000000000008".to_string());
    extra_test.id = "TEST-REVIEW-2".to_string();
    files.tests.tests.push(extra_test);
    let changed = graph_from_trace_files(&files).expect("adapt with extra test");
    assert_eq!(
        review_content_digest_v1(&changed.review_content(LLR_UID).expect("llr projects")),
        review_content_digest_v1(
            &legacy_graph()
                .review_content(LLR_UID)
                .expect("llr projects")
        ),
        "adding a test mapping must not change the verified requirement's digest"
    );

    // Moving records between indexed files leaves the digest alone.
    const RECORD_D: &str = r#"
[[requirements]]
uid = "req_00000000-0000-4000-8000-00000000000d"
id = "R-D"
layer = "sys"
title = "root"
description = "root prose"
"#;
    const RECORD_E: &str = r#"
[[requirements]]
uid = "req_00000000-0000-4000-8000-00000000000e"
id = "R-E"
layer = "hlr"
title = "leaf"
derives_from = ["req_00000000-0000-4000-8000-00000000000d"]
"#;
    let single = tempfile::tempdir().expect("tempdir");
    write(
        &single.path().join("all/records.toml"),
        &format!("schema_version = 1\n{RECORD_D}{RECORD_E}"),
    );
    write(
        &single.path().join("corpus.toml"),
        "schema_version = 1\nrequirements = [\"all/**/*.toml\"]\n",
    );
    let split = tempfile::tempdir().expect("tempdir");
    write(
        &split.path().join("x/one.toml"),
        &format!("schema_version = 1\n{RECORD_D}"),
    );
    write(
        &split.path().join("y/two.toml"),
        &format!("schema_version = 1\n{RECORD_E}"),
    );
    write(
        &split.path().join("corpus.toml"),
        "schema_version = 1\nrequirements = [\"x/**/*.toml\", \"y/**/*.toml\"]\n",
    );
    let single_graph =
        CorpusIndex::load_graph(&single.path().join("corpus.toml")).expect("load single layout");
    let split_graph =
        CorpusIndex::load_graph(&split.path().join("corpus.toml")).expect("load split layout");
    for uid in [
        "req_00000000-0000-4000-8000-00000000000d",
        "req_00000000-0000-4000-8000-00000000000e",
    ] {
        assert_eq!(
            review_content_digest_v1(&single_graph.review_content(uid).expect("single projects")),
            review_content_digest_v1(&split_graph.review_content(uid).expect("split projects")),
            "moving a record between files must not change its digest"
        );
    }
}

fn write(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent");
    }
    fs::write(path, content).expect("write file");
}
