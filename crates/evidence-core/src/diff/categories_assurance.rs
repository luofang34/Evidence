//! Assurance-state categories for [`compare_bundles`]:
//! `objective_mappings`, `reviews_approvals`, `anomalies`,
//! `tool_identity`, `integrity`, `completeness_states`,
//! `content_hash`.

use std::collections::BTreeMap;

use serde_json::Value;

use super::categories_content::{presence_label, status_of, value_label};
use super::{
    CategoryDiff, DiffCategoryStatus, DiffError, Load, Side, file_exists, read_json_file,
    require_indexes, unverifiable,
};

/// Load `compliance/*.json` as raw JSON values keyed by file name.
/// Raw values (not `ComplianceReport`) so the `standards_pack`
/// field — `#[serde(skip_deserializing)]` on the typed report —
/// still compares by its recorded identity.
fn load_compliance_reports(side: &Side) -> Result<BTreeMap<String, Load<Value>>, DiffError> {
    let dir = side.root.join("compliance");
    let mut out = BTreeMap::new();
    if !dir.is_dir() {
        return Ok(out);
    }
    let entries = std::fs::read_dir(&dir).map_err(|source| DiffError::Io {
        path: dir.clone(),
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| DiffError::Io {
            path: dir.clone(),
            source,
        })?;
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.ends_with(".json") {
            continue;
        }
        let rel = format!("compliance/{name}");
        out.insert(name, read_json_file::<Value>(&side.root, &rel)?);
    }
    Ok(out)
}

/// `objective_mappings` — per-crate objective statuses plus the
/// raw standards-pack identity, from `compliance/<crate>.json`.
/// The directory's absence is unverifiable (not equal) whenever
/// either side's `dal_map` declares in-scope crates: the mapping
/// claim cannot be examined.
pub(crate) fn objective_mappings(a: &Side, b: &Side) -> Result<CategoryDiff, DiffError> {
    let reports_a = load_compliance_reports(a)?;
    let reports_b = load_compliance_reports(b)?;

    if reports_a.is_empty() || reports_b.is_empty() {
        // Both empty is equal only when neither side claims a scope
        // — an undeveloped dev bundle legitimately has no reports.
        let dal_empty = |s: &Side| match &s.index {
            Load::Ok(index) => index.dal_map.is_empty(),
            _ => false,
        };
        if reports_a.is_empty() && reports_b.is_empty() && dal_empty(a) && dal_empty(b) {
            return Ok(CategoryDiff {
                category: "objective_mappings",
                status: DiffCategoryStatus::Equal,
                details: vec!["no compliance reports on either side".to_string()],
            });
        }
        let which = match (reports_a.is_empty(), reports_b.is_empty()) {
            (true, true) => "both bundles",
            (true, false) => "bundle A",
            (false, true) => "bundle B",
            (false, false) => "neither bundle",
        };
        return Ok(unverifiable(
            "objective_mappings",
            format!("compliance/ absent or empty in {which}"),
        ));
    }

    let mut details = Vec::new();
    let mut changed = false;
    let mut unexaminable = false;
    for name in reports_a.keys().chain(reports_b.keys()) {
        let (ra, rb) = (reports_a.get(name), reports_b.get(name));
        match (ra, rb) {
            (Some(Load::Ok(_)), None) => {
                changed = true;
                details.push(format!("- compliance/{name}"));
            }
            (None, Some(Load::Ok(_))) => {
                changed = true;
                details.push(format!("+ compliance/{name}"));
            }
            (Some(Load::Unparseable), _) => {
                unexaminable = true;
                details.push(format!("! compliance/{name} unparseable in bundle A"));
            }
            (_, Some(Load::Unparseable)) => {
                unexaminable = true;
                details.push(format!("! compliance/{name} unparseable in bundle B"));
            }
            (None, None) | (Some(Load::Missing), _) | (_, Some(Load::Missing)) => {}
            (Some(Load::Ok(va)), Some(Load::Ok(vb))) => {
                compare_report(name, va, vb, &mut details, &mut changed);
            }
        }
    }

    Ok(CategoryDiff {
        category: "objective_mappings",
        status: status_of(changed, unexaminable),
        details,
    })
}

/// Compare one crate's report between sides: DAL labels, raw
/// standards-pack identity, and per-objective statuses.
fn compare_report(
    name: &str,
    va: &Value,
    vb: &Value,
    details: &mut Vec<String>,
    changed: &mut bool,
) {
    let krate = name.trim_end_matches(".json");
    for field in ["dal", "assurance_level"] {
        let (fa, fb) = (
            va.get(field).cloned().unwrap_or(Value::Null),
            vb.get(field).cloned().unwrap_or(Value::Null),
        );
        if fa != fb {
            *changed = true;
            details.push(format!(
                "~ {krate} {field}: {} -> {}",
                value_label(&fa),
                value_label(&fb)
            ));
        }
    }
    let (pa, pb) = (
        va.get("standards_pack").cloned().unwrap_or(Value::Null),
        vb.get("standards_pack").cloned().unwrap_or(Value::Null),
    );
    if pa != pb {
        *changed = true;
        details.push(format!("~ {krate} standards_pack identity changed"));
    }

    let objectives = |v: &Value| -> BTreeMap<String, String> {
        v.get("objectives")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(|o| {
                        let id = o.get("objective_id")?.as_str()?.to_string();
                        let status = o
                            .get("status")
                            .map(value_label)
                            .unwrap_or_else(|| "(none)".to_string());
                        Some((id, status))
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    let oa = objectives(va);
    let ob = objectives(vb);
    for id in oa
        .keys()
        .chain(ob.keys())
        .collect::<std::collections::BTreeSet<_>>()
    {
        match (oa.get(id), ob.get(id)) {
            (Some(sa), Some(sb)) if sa != sb => {
                *changed = true;
                details.push(format!("~ {krate} {id}: {sa} -> {sb}"));
            }
            (Some(sa), None) => {
                *changed = true;
                details.push(format!("- {krate} {id}: {sa}"));
            }
            (None, Some(sb)) => {
                *changed = true;
                details.push(format!("+ {krate} {id}: {sb}"));
            }
            _ => {}
        }
    }
}

/// `reviews_approvals` — always Unverifiable. Review and approval
/// records are workspace-corpus state, not bundle artifacts (the
/// bundle binding is M6 cutover work), so no bundle pair can be
/// compared on them from bundle content alone. The status is the
/// honest answer, not a gap in the comparison.
pub(crate) fn reviews_approvals() -> CategoryDiff {
    unverifiable(
        "reviews_approvals",
        "reviews/approvals are workspace corpus state, not bundle artifacts (M6 binding); \
         the category cannot be compared from bundle content"
            .to_string(),
    )
}

/// `anomalies` — the recorded tool-command failures
/// (`index.tool_command_failures`), the in-bundle anomaly proxy.
pub(crate) fn anomalies(a: &Side, b: &Side) -> CategoryDiff {
    let (ia, ib) = match require_indexes(a, b, "anomalies") {
        Ok(pair) => pair,
        Err(diff) => return diff,
    };
    let rows = |index: &crate::bundle::EvidenceIndex| -> BTreeMap<(String, i32), Vec<String>> {
        let mut map: BTreeMap<(String, i32), Vec<String>> = BTreeMap::new();
        for f in &index.tool_command_failures {
            map.entry((f.command_name.clone(), f.exit_code))
                .or_default()
                .push(f.stderr_tail.clone());
        }
        for tails in map.values_mut() {
            tails.sort();
        }
        map
    };
    let (ra, rb) = (rows(ia), rows(ib));
    let mut details = Vec::new();
    for key in ra
        .keys()
        .chain(rb.keys())
        .collect::<std::collections::BTreeSet<_>>()
    {
        let (name, code) = key;
        match (ra.get(key), rb.get(key)) {
            (Some(ta), Some(tb)) if ta != tb => {
                details.push(format!("~ {name} exit {code} stderr_tail changed"));
            }
            (Some(_), None) => details.push(format!("- {name} exit {code}")),
            (None, Some(_)) => details.push(format!("+ {name} exit {code}")),
            _ => {}
        }
    }
    CategoryDiff {
        category: "anomalies",
        status: if details.is_empty() {
            DiffCategoryStatus::Equal
        } else {
            DiffCategoryStatus::Changed
        },
        details,
    }
}

/// `tool_identity` — engine provenance (crate version, git sha,
/// build source), the `env.json` toolchain fields, and the
/// standards-pack identity set across the compliance reports.
pub(crate) fn tool_identity(a: &Side, b: &Side) -> Result<CategoryDiff, DiffError> {
    let (ia, ib) = match require_indexes(a, b, "tool_identity") {
        Ok(pair) => pair,
        Err(diff) => return Ok(diff),
    };
    let mut details = Vec::new();
    let mut changed = false;
    let mut unexaminable = false;

    let va = serde_json::to_value(ia).unwrap_or(Value::Null);
    let vb = serde_json::to_value(ib).unwrap_or(Value::Null);
    for field in [
        "engine_crate_version",
        "engine_git_sha",
        "engine_build_source",
    ] {
        let (fa, fb) = (
            va.get(field).cloned().unwrap_or(Value::Null),
            vb.get(field).cloned().unwrap_or(Value::Null),
        );
        if fa != fb {
            changed = true;
            details.push(format!(
                "~ {field}: {} -> {}",
                value_label(&fa),
                value_label(&fb)
            ));
        }
    }

    let env_a = read_json_file::<Value>(&a.root, "env.json")?;
    let env_b = read_json_file::<Value>(&b.root, "env.json")?;
    match (&env_a, &env_b) {
        (Load::Ok(ea), Load::Ok(eb)) => {
            for field in [
                "rustc",
                "cargo",
                "llvm_version",
                "target_triple",
                "tool_prerelease",
            ] {
                let (fa, fb) = (
                    ea.get(field).cloned().unwrap_or(Value::Null),
                    eb.get(field).cloned().unwrap_or(Value::Null),
                );
                if fa != fb {
                    changed = true;
                    details.push(format!(
                        "~ {field}: {} -> {}",
                        value_label(&fa),
                        value_label(&fb)
                    ));
                }
            }
        }
        (state_a, state_b) => {
            unexaminable = true;
            for (side, state) in [("A", state_a), ("B", state_b)] {
                if !matches!(state, Load::Ok(_)) {
                    details.push(format!(
                        "! env.json {} in bundle {side}",
                        state.state_label()
                    ));
                }
            }
        }
    }

    // Standards-pack identity set across every compliance report.
    let pack_set = |reports: &BTreeMap<String, Load<Value>>| -> std::collections::BTreeSet<String> {
        reports
            .values()
            .filter_map(|load| match load {
                Load::Ok(v) => v.get("standards_pack").cloned(),
                _ => None,
            })
            .map(|v| v.to_string())
            .collect()
    };
    let (pa, pb) = (
        pack_set(&load_compliance_reports(a)?),
        pack_set(&load_compliance_reports(b)?),
    );
    if pa != pb {
        changed = true;
        details.push("~ standards_pack identity set changed".to_string());
    }

    Ok(CategoryDiff {
        category: "tool_identity",
        status: status_of(changed, unexaminable),
        details,
    })
}

/// `integrity` — the signature and hash-manifest presence. The
/// whole-content hash value itself is the `content_hash` category.
pub(crate) fn integrity(a: &Side, b: &Side) -> CategoryDiff {
    let mut details = Vec::new();
    for (label, rel) in [
        ("signature (BUNDLE.sig)", "BUNDLE.sig"),
        ("SHA256SUMS", "SHA256SUMS"),
    ] {
        let (pa, pb) = (file_exists(a, rel), file_exists(b, rel));
        if pa != pb {
            details.push(format!(
                "~ {label}: {} -> {}",
                presence_label(pa),
                presence_label(pb)
            ));
        }
    }
    CategoryDiff {
        category: "integrity",
        status: if details.is_empty() {
            DiffCategoryStatus::Equal
        } else {
            DiffCategoryStatus::Changed
        },
        details,
    }
}

/// `completeness_states` — the per-area derived states recorded on
/// `index.completeness`, compared area by area.
pub(crate) fn completeness_states(a: &Side, b: &Side) -> CategoryDiff {
    let (ia, ib) = match require_indexes(a, b, "completeness_states") {
        Ok(pair) => pair,
        Err(diff) => return diff,
    };
    let va = serde_json::to_value(&ia.completeness).unwrap_or(Value::Null);
    let vb = serde_json::to_value(&ib.completeness).unwrap_or(Value::Null);
    let mut details = Vec::new();
    for area in [
        "capture",
        "graph_validity",
        "verification",
        "objective_mapping",
        "review_approval",
        "integrity",
        "reproducibility",
        "tool_qualification",
    ] {
        let (fa, fb) = (
            va.get(area).cloned().unwrap_or(Value::Null),
            vb.get(area).cloned().unwrap_or(Value::Null),
        );
        if fa != fb {
            details.push(format!(
                "~ {area}: {} -> {}",
                value_label(&fa),
                value_label(&fb)
            ));
        }
    }
    CategoryDiff {
        category: "completeness_states",
        status: if details.is_empty() {
            DiffCategoryStatus::Equal
        } else {
            DiffCategoryStatus::Changed
        },
        details,
    }
}

/// `content_hash` — the whole-content backstop: any content-layer
/// byte difference between the two bundles lands here even when a
/// category above failed to localize it.
pub(crate) fn content_hash(a: &Side, b: &Side) -> CategoryDiff {
    let (ia, ib) = match require_indexes(a, b, "content_hash") {
        Ok(pair) => pair,
        Err(diff) => return diff,
    };
    let details = if ia.content_hash == ib.content_hash {
        Vec::new()
    } else {
        vec![format!(
            "~ content_hash: {} -> {}",
            ia.content_hash, ib.content_hash
        )]
    };
    CategoryDiff {
        category: "content_hash",
        status: if details.is_empty() {
            DiffCategoryStatus::Equal
        } else {
            DiffCategoryStatus::Changed
        },
        details,
    }
}
