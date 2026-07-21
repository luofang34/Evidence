//! Tests for strict source-revision record loading: round-trip of
//! every capture shape and fail-closed schema and field validation
//! (TEST-141).

use super::records::validate_vendored_wire_path;
use super::tests_support::*;
use crate::corpus::{CorpusGraph, SourceError, VendoredPathRule};

/// Vendored, hash-only, external-controlled, and unavailable
/// records all load into typed nodes with every field preserved —
/// the canonical location byte-exact, accepted text untrimmed, and
/// an uppercase media type accepted (TEST-141).
#[test]
fn four_record_shapes_round_trip_into_typed_nodes() {
    let mut hash_only = vendored(SRC_2, "SRC-2");
    hash_only.material_toml = format!(
        "state = \"available\"\nretrieved_at = \"2026-07-02T11:00:00Z\"\nsha256 = \"{DIGEST}\"\n\n[sources.material.capture]\nmode = \"hash_only\"\n"
    );
    let mut external = vendored(SRC_3, "SRC-3");
    external.material_toml = format!(
        "state = \"available\"\nretrieved_at = \"2026-07-03T12:00:00Z\"\nsha256 = \"{DIGEST}\"\n\n[sources.material.capture]\nmode = \"external_controlled\"\nsystem = \"SharePoint\"\nimmutable_id = \"doc-1-rev-c\"\n"
    );
    let mut unavailable = vendored(SRC_4, "SRC-4");
    unavailable.material_toml =
        "state = \"unavailable\"\nreason = \"upstream returned 404\"".to_string();

    // Accepted text is stored untrimmed; the canonical location is
    // preserved exactly, never normalized; uppercase media types
    // are RFC-case-insensitive and valid.
    let mut padded = vendored(SRC_1, "SRC-1");
    padded.title = "  padded title  ".to_string();
    padded.canonical_location = "HTTPS://Example.ORG/specs/../DOC-1?x=1#frag".to_string();
    padded.media_type = "Application/PDF".to_string();

    let graph = load_source_content(&source_file(&[
        padded.clone(),
        hash_only,
        external,
        unavailable,
    ]))
    .expect("all four shapes load");
    assert_eq!(graph.len(), 4);

    let vendored_node = expect_source(&graph, SRC_1);
    assert_eq!(vendored_node.id, "SRC-1");
    assert_eq!(vendored_node.document_key, "DOC-1");
    assert_eq!(vendored_node.title, "  padded title  ");
    assert_eq!(vendored_node.media_type, "Application/PDF");
    assert_eq!(
        vendored_node.canonical_location,
        "HTTPS://Example.ORG/specs/../DOC-1?x=1#frag"
    );
    assert!(vendored_node.edges.is_empty());
    match &vendored_node.material {
        crate::corpus::SourceMaterial::Available {
            retrieved_at,
            sha256,
            capture,
        } => {
            assert_eq!(retrieved_at, "2026-07-01T10:00:00Z");
            assert_eq!(sha256.as_str(), DIGEST);
            assert!(
                matches!(
                    capture,
                    crate::corpus::SourceCapture::Vendored { path }
                        if path == "sources/doc-1/rev-c.pdf"
                ),
                "vendored capture must carry its path, got: {capture:?}"
            );
        }
        other => panic!("expected available material, got: {other:?}"),
    }

    let hash_only_node = expect_source(&graph, SRC_2);
    assert!(
        matches!(
            hash_only_node.material,
            crate::corpus::SourceMaterial::Available {
                capture: crate::corpus::SourceCapture::HashOnly {},
                ..
            }
        ),
        "hash-only capture must round-trip: {:?}",
        hash_only_node.material
    );

    let external_node = expect_source(&graph, SRC_3);
    assert!(
        matches!(
            &external_node.material,
            crate::corpus::SourceMaterial::Available {
                capture: crate::corpus::SourceCapture::ExternalControlled { system, immutable_id },
                ..
            } if system == "SharePoint" && immutable_id == "doc-1-rev-c"
        ),
        "external-controlled capture must round-trip: {:?}",
        external_node.material
    );

    let unavailable_node = expect_source(&graph, SRC_4);
    assert!(
        matches!(
            &unavailable_node.material,
            crate::corpus::SourceMaterial::Unavailable { reason } if reason == "upstream returned 404"
        ),
        "unavailable material must round-trip: {:?}",
        unavailable_node.material
    );
}

/// Unknown fields anywhere in the schema, a newer schema version,
/// unknown state or mode tags, and malformed digests all fail at
/// parse or schema gating (TEST-141).
#[test]
fn strict_schema_violations_fail_closed() {
    let cases: [(&str, String); 6] = [
        (
            "unknown top-level field",
            "schema_version = 1\nfrobnicate = true\n".to_string(),
        ),
        (
            "unknown record field",
            source_file(&[vendored(SRC_1, "SRC-1")])
                .replace("title =", "ingester = \"tool-x\"\ntitle ="),
        ),
        (
            "unknown material field",
            source_file(&[vendored(SRC_1, "SRC-1")]).replace(
                "state = \"available\"",
                "state = \"available\"\nrecipe = \"pdf-v3\"",
            ),
        ),
        (
            "unknown state tag",
            source_file(&[vendored(SRC_1, "SRC-1")])
                .replace("state = \"available\"", "state = \"pending\""),
        ),
        (
            "unknown capture mode",
            source_file(&[vendored(SRC_1, "SRC-1")])
                .replace("mode = \"vendored\"", "mode = \"teleported\""),
        ),
        (
            "unknown capture field",
            source_file(&[vendored(SRC_1, "SRC-1")]).replace(
                "path = \"sources/doc-1/rev-c.pdf\"",
                "path = \"sources/doc-1/rev-c.pdf\"\nworkstream = \"p1\"",
            ),
        ),
    ];
    for (case, content) in cases {
        let err = expect_load_err(load_source_content(&content), case);
        assert!(
            matches!(err, SourceError::RecordParse { .. }),
            "{case} must fail at parse, got: {err:?}"
        );
    }

    let err = expect_load_err(
        load_source_content("schema_version = 2\n"),
        "newer schema version",
    );
    assert!(
        matches!(
            err,
            SourceError::RecordSchemaTooNew {
                found: 2,
                supported: 1,
                ..
            }
        ),
        "schema_version 2 must fail closed, got: {err:?}"
    );

    for (case, bad_digest) in [
        ("uppercase digest", DIGEST.to_uppercase()),
        ("short digest", DIGEST[..63].to_string()),
        ("non-hex digest", format!("{}g", &DIGEST[..63])),
    ] {
        let content = source_file(&[vendored(SRC_1, "SRC-1")]).replace(DIGEST, &bad_digest);
        let err = expect_load_err(load_source_content(&content), case);
        assert!(
            matches!(err, SourceError::RecordParse { .. }),
            "{case} must fail at parse through the validating digest, got: {err:?}"
        );
    }
}

/// Malformed uids, blank required strings, malformed media types,
/// and malformed timestamps each fail with a typed error naming the
/// file path and the record identity (TEST-141).
#[test]
fn malformed_identity_and_field_values_fail_closed() {
    for (case, bad_uid) in [
        ("missing prefix", "00000000-0000-4000-8000-0000000000a1"),
        ("not a uuid", "src_not-a-uuid"),
        ("not v4", "src_c2f6c0e0-5e3a-11ec-8d3d-0242ac130003"),
        (
            "not RFC4122 variant",
            "src_00000000-0000-4000-c000-000000000000",
        ),
    ] {
        let mut spec = vendored(SRC_1, "SRC-1");
        spec.uid = bad_uid.to_string();
        let err = expect_load_err(load_source_content(&source_file(&[spec])), case);
        assert!(
            matches!(
                err,
                SourceError::NativeUidPrefix { .. } | SourceError::NativeUidUuidV4 { .. }
            ),
            "{case} must fail native uid validation, got: {err:?}"
        );
    }

    let blank_string_cases: [(&str, RecordSpec); 7] = [
        ("blank id", {
            let mut spec = vendored(SRC_1, "SRC-1");
            spec.id = " \t".to_string();
            spec
        }),
        ("blank document_key", {
            let mut spec = vendored(SRC_1, "SRC-1");
            spec.document_key = "   ".to_string();
            spec
        }),
        ("blank title", {
            let mut spec = vendored(SRC_1, "SRC-1");
            spec.title = "   ".to_string();
            spec
        }),
        ("blank canonical_location", {
            let mut spec = vendored(SRC_1, "SRC-1");
            spec.canonical_location = "   ".to_string();
            spec
        }),
        ("blank reason", {
            let mut spec = vendored(SRC_1, "SRC-1");
            spec.material_toml = "state = \"unavailable\"\nreason = \"   \"".to_string();
            spec
        }),
        ("blank system", {
            let mut spec = vendored(SRC_1, "SRC-1");
            spec.material_toml = format!(
                "state = \"available\"\nretrieved_at = \"2026-07-01T10:00:00Z\"\nsha256 = \"{DIGEST}\"\n\n[sources.material.capture]\nmode = \"external_controlled\"\nsystem = \"   \"\nimmutable_id = \"doc-1-rev-c\"\n"
            );
            spec
        }),
        ("blank immutable_id", {
            let mut spec = vendored(SRC_1, "SRC-1");
            spec.material_toml = format!(
                "state = \"available\"\nretrieved_at = \"2026-07-01T10:00:00Z\"\nsha256 = \"{DIGEST}\"\n\n[sources.material.capture]\nmode = \"external_controlled\"\nsystem = \"SharePoint\"\nimmutable_id = \"  \"\n"
            );
            spec
        }),
    ];
    for (case, spec) in blank_string_cases {
        let err = expect_load_err(load_source_content(&source_file(&[spec])), case);
        let matched = match case {
            "blank id" => matches!(err, SourceError::SourceHumanId { .. }),
            "blank document_key" => matches!(err, SourceError::SourceDocumentKey { .. }),
            "blank title" => matches!(err, SourceError::SourceTitle { .. }),
            "blank canonical_location" => {
                matches!(err, SourceError::SourceCanonicalLocation { .. })
            }
            "blank reason" => matches!(err, SourceError::SourceReason { .. }),
            "blank system" => matches!(err, SourceError::SourceCaptureSystem { .. }),
            "blank immutable_id" => matches!(err, SourceError::SourceCaptureImmutableId { .. }),
            other => unreachable!("unexpected case {other}"),
        };
        assert!(
            matched,
            "{case} must fail with its typed variant, got: {err:?}"
        );
    }

    let mut spec = vendored(SRC_1, "SRC-1");
    spec.material_toml = spec
        .material_toml
        .replace("2026-07-01T10:00:00Z", "not-a-timestamp");
    let err = expect_load_err(
        load_source_content(&source_file(&[spec])),
        "malformed timestamp",
    );
    assert!(
        matches!(
            err,
            SourceError::SourceTimestamp { ref value, .. } if value == "not-a-timestamp"
        ),
        "a malformed retrieved_at must fail with SourceTimestamp, got: {err:?}"
    );

    let media_cases = [
        "applicationpdf",
        "application/",
        "/pdf",
        "application /pdf",
        "application/pd f",
        "application/pdf; charset=utf-8",
        "application/pdf/",
    ];
    for bad_media in media_cases {
        let mut spec = vendored(SRC_1, "SRC-1");
        spec.media_type = bad_media.to_string();
        let err = expect_load_err(
            load_source_content(&source_file(&[spec])),
            "malformed media type",
        );
        assert!(
            matches!(
                err,
                SourceError::SourceMediaType { ref value, .. } if value == bad_media
            ),
            "media type {bad_media:?} must fail with SourceMediaType, got: {err:?}"
        );
    }

    // The record-context variants name the file path and the record.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("context.toml");
    let mut spec = vendored(SRC_1, "SRC-1");
    spec.title = "   ".to_string();
    write(&path, &source_file(&[spec]));
    let mut graph = CorpusGraph::new();
    let err = match super::records::load_sources_into(&path, &mut graph) {
        Ok(()) => panic!("blank title must fail closed"),
        Err(err) => err,
    };
    assert!(
        matches!(
            err,
            SourceError::SourceTitle { path: ref err_path, ref uid, ref id }
                if *err_path == path && uid == SRC_1 && id == "SRC-1"
        ),
        "the typed error must name path, uid, and id, got: {err:?}"
    );
}

/// Incomplete or impossible material and capture combinations fail
/// deserialization — they never load as valid records (TEST-141).
#[test]
fn incomplete_capture_combinations_fail_closed() {
    let base = source_file(&[vendored(SRC_1, "SRC-1")]);
    let unavailable = format!(
        "state = \"available\"\nretrieved_at = \"2026-07-01T10:00:00Z\"\nsha256 = \"{DIGEST}\"\n\n[sources.material.capture]\nmode = \"vendored\"\npath = \"sources/doc-1/rev-c.pdf\""
    );
    let cases: [(&str, String); 10] = [
        (
            "available missing retrieved_at",
            base.replace("retrieved_at = \"2026-07-01T10:00:00Z\"\n", ""),
        ),
        (
            "available missing sha256",
            base.replace(&format!("sha256 = \"{DIGEST}\"\n"), ""),
        ),
        (
            "available missing capture",
            base.replace(
                "\n[sources.material.capture]\nmode = \"vendored\"\npath = \"sources/doc-1/rev-c.pdf\"\n",
                "",
            ),
        ),
        (
            "vendored missing path",
            base.replace("path = \"sources/doc-1/rev-c.pdf\"\n", ""),
        ),
        (
            "external missing immutable_id",
            base.replace(
                "mode = \"vendored\"\npath = \"sources/doc-1/rev-c.pdf\"",
                "mode = \"external_controlled\"\nsystem = \"SharePoint\"",
            ),
        ),
        (
            "external missing system",
            base.replace(
                "mode = \"vendored\"\npath = \"sources/doc-1/rev-c.pdf\"",
                "mode = \"external_controlled\"\nimmutable_id = \"doc-1-rev-c\"",
            ),
        ),
        (
            "unavailable with a digest",
            base.replace(
                &unavailable,
                &format!(
                    "state = \"unavailable\"\nreason = \"gone\"\nsha256 = \"{DIGEST}\""
                ),
            ),
        ),
        (
            "unavailable with retrieved_at",
            base.replace(
                &unavailable,
                "state = \"unavailable\"\nreason = \"gone\"\nretrieved_at = \"2026-07-01T10:00:00Z\"",
            ),
        ),
        (
            "unavailable missing reason",
            base.replace(&unavailable, "state = \"unavailable\""),
        ),
        (
            "hash_only with a stray path",
            base.replace("mode = \"vendored\"", "mode = \"hash_only\""),
        ),
    ];
    for (case, content) in cases {
        let err = expect_load_err(load_source_content(&content), case);
        assert!(
            matches!(err, SourceError::RecordParse { .. }),
            "{case} must fail deserialization, got: {err:?}"
        );
    }
}

/// Every unsafe or non-canonical vendored wire path fails the
/// lexical rule check, and the record-level error carries the
/// offending value and the violated rule (TEST-141).
#[test]
fn vendored_paths_reject_unsafe_wire_forms() {
    let cases: [(&str, VendoredPathRule); 13] = [
        ("", VendoredPathRule::Empty),
        ("/x", VendoredPathRule::Absolute),
        ("C:\\x", VendoredPathRule::DrivePrefix),
        ("C:/x", VendoredPathRule::DrivePrefix),
        ("c:x", VendoredPathRule::DrivePrefix),
        ("\\\\server\\x", VendoredPathRule::UncPrefix),
        ("a\\b", VendoredPathRule::Backslash),
        ("a//b", VendoredPathRule::EmptyComponent),
        ("/a//b", VendoredPathRule::Absolute),
        ("a/./b", VendoredPathRule::DotComponent),
        (".", VendoredPathRule::DotComponent),
        ("a/../b", VendoredPathRule::ParentComponent),
        ("..", VendoredPathRule::ParentComponent),
    ];
    for (path, rule) in cases {
        let err = validate_vendored_wire_path(path).expect_err("unsafe path must be rejected");
        assert_eq!(err, rule, "path {path:?}");
    }
    for valid in ["a", "sources/doc-1/rev-c.pdf", "a/b/c-1.2_3+4.pdf"] {
        assert!(
            validate_vendored_wire_path(valid).is_ok(),
            "path {valid:?} must be accepted"
        );
    }

    let mut spec = vendored(SRC_1, "SRC-1");
    spec.material_toml = spec
        .material_toml
        .replace("sources/doc-1/rev-c.pdf", "../escape");
    let err = expect_load_err(
        load_source_content(&source_file(&[spec])),
        "parent-component path",
    );
    assert!(
        matches!(
            err,
            SourceError::SourceVendoredPath { ref value, rule, .. }
                if value == "../escape" && rule == VendoredPathRule::ParentComponent
        ),
        "the record error must carry value and rule, got: {err:?}"
    );
}
