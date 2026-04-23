use super::*;

fn hlr_entry(uid: Option<&str>) -> HlrEntry {
    HlrEntry {
        uid: uid.map(|s| s.to_string()),
        ns: None,
        id: "HLR-X".to_string(),
        title: "fixture".to_string(),
        owner: None,
        scope: None,
        sort_key: None,
        category: None,
        source: None,
        description: None,
        rationale: None,
        verification_methods: vec![],
        traces_to: vec![],
        surfaces: vec![],
    }
}

#[test]
fn assign_valid_uuids_hlr_fills_missing_and_invalid() {
    let mut entries = vec![
        hlr_entry(None),
        hlr_entry(Some("HLR-001")),           // placeholder string
        hlr_entry(Some("not-a-uuid-either")), // any non-UUID string
        hlr_entry(Some("5c6e07f1-da4a-4aec-9647-426304deadb5")), // valid
    ];
    let (count, remap) = assign_valid_uuids_hlr(&mut entries);
    assert_eq!(count, 3, "3 entries should be rewritten");
    // Remap only contains entries whose old uid was Some(_).
    assert_eq!(remap.len(), 2);
    assert!(remap.contains_key("HLR-001"));
    assert!(remap.contains_key("not-a-uuid-either"));
    // Every rewritten entry now carries a valid UUID.
    for entry in entries.iter().take(3) {
        let s = entry.uid.as_deref().unwrap();
        assert!(uuid::Uuid::parse_str(s).is_ok(), "not a uuid: {s}");
    }
    // The already-valid entry keeps its uid.
    assert_eq!(
        entries[3].uid.as_deref(),
        Some("5c6e07f1-da4a-4aec-9647-426304deadb5")
    );
}

#[test]
fn rewrite_traces_to_applies_remap() {
    let remap: BTreeMap<String, String> = [
        ("HLR-001".to_string(), "aaaa-new-uuid".to_string()),
        ("HLR-002".to_string(), "bbbb-new-uuid".to_string()),
    ]
    .into_iter()
    .collect();
    let mut refs = vec![
        "HLR-001".to_string(),
        "something-else".to_string(),
        "HLR-002".to_string(),
    ];
    let changed = rewrite_traces_to(&mut refs, &remap);
    assert!(changed);
    assert_eq!(refs[0], "aaaa-new-uuid");
    assert_eq!(refs[1], "something-else");
    assert_eq!(refs[2], "bbbb-new-uuid");
}

#[test]
fn rewrite_traces_to_noop_when_no_match() {
    let remap: BTreeMap<String, String> = BTreeMap::new();
    let mut refs = vec!["HLR-001".to_string()];
    let changed = rewrite_traces_to(&mut refs, &remap);
    assert!(!changed);
    assert_eq!(refs[0], "HLR-001");
}

#[test]
fn assign_valid_uuids_derived_rewrites_invalid() {
    let mut entries = vec![
        DerivedEntry {
            uid: None,
            id: "DER-001".to_string(),
            title: "Derived req".to_string(),
            owner: None,
            source: None,
            description: None,
            rationale: None,
            safety_impact: None,
            sort_key: None,
        },
        DerivedEntry {
            uid: Some("DRQ-001".to_string()),
            id: "DER-002".to_string(),
            title: "Placeholder uid".to_string(),
            owner: None,
            source: None,
            description: None,
            rationale: None,
            safety_impact: None,
            sort_key: None,
        },
    ];
    let (count, remap) = assign_valid_uuids_derived(&mut entries);
    assert_eq!(count, 2);
    assert_eq!(remap.len(), 1, "only non-None olds go into remap");
    assert!(remap.contains_key("DRQ-001"));
    for entry in &entries {
        let s = entry.uid.as_deref().unwrap();
        assert!(uuid::Uuid::parse_str(s).is_ok(), "not a uuid: {s}");
    }
}

#[test]
#[should_panic(expected = "duplicate placeholder uid across layers")]
fn merge_remap_panics_on_cross_layer_collision() {
    let mut dst: BTreeMap<String, String> =
        [("shared-placeholder".to_string(), "first-uuid".to_string())]
            .into_iter()
            .collect();
    let src: BTreeMap<String, String> =
        [("shared-placeholder".to_string(), "second-uuid".to_string())]
            .into_iter()
            .collect();
    merge_remap(&mut dst, src);
}

#[test]
fn merge_remap_non_overlapping_keys_combine_cleanly() {
    let mut dst: BTreeMap<String, String> =
        [("a".to_string(), "aa".to_string())].into_iter().collect();
    let src: BTreeMap<String, String> = [("b".to_string(), "bb".to_string())].into_iter().collect();
    merge_remap(&mut dst, src);
    assert_eq!(dst.len(), 2);
    assert_eq!(dst["a"], "aa");
    assert_eq!(dst["b"], "bb");
}

#[test]
fn needs_new_uuid_accepts_only_valid_uuids() {
    assert!(needs_new_uuid(None));
    assert!(needs_new_uuid(Some("")));
    assert!(needs_new_uuid(Some("HLR-001")));
    assert!(needs_new_uuid(Some("not-quite-uuid-shape")));
    assert!(!needs_new_uuid(Some(
        "5c6e07f1-da4a-4aec-9647-426304deadb5"
    )));
}
