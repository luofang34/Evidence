//! Facade-level ingestion tests (TEST-176, TEST-179): the recipe
//! identity, the input contract's fail-closed gates, and the
//! independence of the recipe, input, and output identity planes.

use super::*;
use crate::hash::sha256;

const REV: &str = "src_00000000-0000-4000-8000-0000000000aa";

fn recipe() -> IngesterRecipe {
    IngesterRecipe {
        parser: "pulldown-cmark".to_string(),
        parser_version: "0.13.4".to_string(),
        extensions: ["footnotes".to_string(), "tables".to_string()]
            .into_iter()
            .collect(),
        adapter_version: "1".to_string(),
        normalization_contract: "evidence/source-node-normalization/v1".to_string(),
    }
}

fn input_for(bytes: &[u8]) -> IngestMarkdownInput<'_> {
    IngestMarkdownInput {
        bytes,
        media_type: "text/markdown",
        source_revision_uid: REV,
        canonical_path: "docs/spec.md",
        input_digest: StructuralContentDigest::from_hasher_output(sha256(bytes)),
        git_blob: None,
        recipe: recipe(),
    }
}

#[test]
fn recipe_canonical_bytes_bind_every_field_deterministically() {
    let base = recipe();
    let bytes = base.canonical_bytes();
    assert_eq!(base.canonical_bytes(), bytes, "encoding is deterministic");
    assert!(
        bytes.starts_with(b"evidence/ingester-recipe/v1\x00"),
        "the encoding opens with its domain tag"
    );

    let mut by_parser = base.clone();
    by_parser.parser = "comrak".to_string();
    let mut by_parser_version = base.clone();
    by_parser_version.parser_version = "0.13.5".to_string();
    let mut by_extensions = base.clone();
    by_extensions.extensions.insert("smart-punct".to_string());
    let mut by_adapter = base.clone();
    by_adapter.adapter_version = "2".to_string();
    let mut by_contract = base.clone();
    by_contract.normalization_contract = "other/v1".to_string();

    for (name, mutated) in [
        ("parser", by_parser),
        ("parser_version", by_parser_version),
        ("extensions", by_extensions),
        ("adapter_version", by_adapter),
        ("normalization_contract", by_contract),
    ] {
        assert_ne!(
            mutated.canonical_bytes(),
            bytes,
            "mutating {name} must move the canonical bytes"
        );
        assert_ne!(
            mutated.digest(),
            base.digest(),
            "mutating {name} must move the recipe digest"
        );
    }
}

#[test]
fn recipe_extension_order_is_non_semantic() {
    let forward = recipe();
    let mut reverse = recipe();
    reverse.extensions = ["tables".to_string(), "footnotes".to_string()]
        .into_iter()
        .collect();
    assert_eq!(forward.canonical_bytes(), reverse.canonical_bytes());
    assert_eq!(forward.digest(), reverse.digest());
}

#[test]
fn recipe_input_and_output_identities_change_independently() {
    let bytes = b"# A\n\nText.\n";
    let base = ingest_markdown(&input_for(bytes)).expect("ingestion succeeds");
    let base_recipe_digest = recipe().digest();

    // Recipe plane: mutating the recipe moves the recipe digest and
    // leaves input and output digests untouched.
    let mut mutated_recipe_input = input_for(bytes);
    mutated_recipe_input.recipe.adapter_version = "2".to_string();
    let rerouted = ingest_markdown(&mutated_recipe_input).expect("ingestion succeeds");
    assert_ne!(mutated_recipe_input.recipe.digest(), base_recipe_digest);
    assert_eq!(rerouted.output_digest, base.output_digest);

    // Input plane: mutating the bytes (with a matching declared
    // digest) moves the input and output digests and leaves the
    // recipe digest untouched.
    let other_bytes = b"# B\n\nText.\n";
    let other = ingest_markdown(&input_for(other_bytes)).expect("ingestion succeeds");
    assert_ne!(other.output_digest, base.output_digest);
    assert_eq!(recipe().digest(), base_recipe_digest);

    // A declared digest that does not match the bytes fails closed:
    // the input identity plane is enforced, not asserted.
    let mut mismatched = input_for(bytes);
    mismatched.input_digest = StructuralContentDigest::from_hasher_output(sha256(other_bytes));
    let outcome = ingest_markdown(&mismatched);
    assert!(
        matches!(outcome, Err(IngestError::InputDigestMismatch { .. })),
        "a digest mismatch must fail closed, got: {outcome:?}"
    );
}

#[test]
fn non_utf8_media_type_and_digest_mismatch_fail_closed() {
    // Non-UTF-8 carries the byte offset of the first invalid
    // sequence; decoding is never lossy.
    let bad_utf8 = b"# H\n\nabc\xffdef\n";
    let outcome = ingest_markdown(&input_for(bad_utf8));
    match outcome {
        Err(IngestError::NonUtf8 { offset }) => assert_eq!(offset, 8),
        other => panic!("expected NonUtf8, got: {other:?}"),
    }

    // Media type mismatch fails closed; the required type compares
    // ASCII case-insensitively.
    let mut wrong_media = input_for(b"# H\n");
    wrong_media.media_type = "text/plain";
    assert!(matches!(
        ingest_markdown(&wrong_media),
        Err(IngestError::MediaTypeMismatch { .. })
    ));
    let mut uppercase_media = input_for(b"# H\n");
    uppercase_media.media_type = "TEXT/MARKDOWN";
    assert!(ingest_markdown(&uppercase_media).is_ok());

    // Revision uid, path, and git blob validate at the boundary.
    let mut bad_uid = input_for(b"# H\n");
    bad_uid.source_revision_uid = "src_not-a-uuid";
    assert!(matches!(
        ingest_markdown(&bad_uid),
        Err(IngestError::InvalidSourceRevisionUid { .. })
    ));
    let mut bad_path = input_for(b"# H\n");
    bad_path.canonical_path = "../escape.md";
    assert!(matches!(
        ingest_markdown(&bad_path),
        Err(IngestError::InvalidCanonicalPath { .. })
    ));
    let mut bad_blob = input_for(b"# H\n");
    bad_blob.git_blob = Some("not-hex".to_string());
    assert!(matches!(
        ingest_markdown(&bad_blob),
        Err(IngestError::InvalidGitBlob { .. })
    ));
}
