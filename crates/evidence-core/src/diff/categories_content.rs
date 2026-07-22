//! Content-plane categories for [`compare_bundles`]: `scope`,
//! `trace_graph`, `recipe`, `inputs`, `outputs`. Split from the
//! facade to keep every file under the 500-line workspace limit;
//! sibling `categories_capture` covers tests / coverage / commands
//! and `categories_assurance` the assurance-state categories.

use std::collections::BTreeMap;

use serde_json::Value;

use super::{
    CategoryDiff, DiffCategoryStatus, DiffError, Load, Side, file_exists, push_field_change,
    require_indexes, unverifiable,
};
use crate::verify::{
    Plane, compare_recipe_fields, diff_digest_planes, read_digest_map, read_recipe,
};

/// Render a JSON scalar for detail lines: strings unquoted,
/// everything else in its compact JSON form.
pub(crate) fn value_label(v: &Value) -> String {
    v.as_str()
        .map(str::to_string)
        .unwrap_or_else(|| v.to_string())
}

/// `scope` — the bundle's claim context: profile, schema versions,
/// git identity, DAL map, boundary policy, resolution policy. All
/// from `index.json`; a missing or unparseable index makes the
/// category unverifiable.
pub(crate) fn scope(a: &Side, b: &Side) -> CategoryDiff {
    let (ia, ib) = match require_indexes(a, b, "scope") {
        Ok(pair) => pair,
        Err(diff) => return diff,
    };
    let va = serde_json::to_value(ia).unwrap_or(Value::Null);
    let vb = serde_json::to_value(ib).unwrap_or(Value::Null);
    let mut details = Vec::new();
    let mut changed = false;

    for field in [
        "profile",
        "schema_version",
        "boundary_schema_version",
        "trace_schema_version",
        "git_sha",
        "git_branch",
        "git_dirty",
        "resolution_policy",
    ] {
        let fa = va.get(field).cloned().unwrap_or(Value::Null);
        let fb = vb.get(field).cloned().unwrap_or(Value::Null);
        if fa != fb {
            changed = true;
            details.push(format!(
                "~ {field}: {} -> {}",
                value_label(&fa),
                value_label(&fb)
            ));
        }
    }

    // `dal_map` key-wise so direction is visible per crate.
    let empty = serde_json::Map::new();
    let dal_a = va
        .get("dal_map")
        .and_then(Value::as_object)
        .unwrap_or(&empty);
    let dal_b = vb
        .get("dal_map")
        .and_then(Value::as_object)
        .unwrap_or(&empty);
    for krate in dal_a
        .keys()
        .chain(dal_b.keys())
        .collect::<std::collections::BTreeSet<_>>()
    {
        match (dal_a.get(krate), dal_b.get(krate)) {
            (Some(la), Some(lb)) => push_field_change(
                &mut details,
                &format!("dal_map[{krate}]"),
                &value_label(la),
                &value_label(lb),
                &mut changed,
            ),
            (Some(la), None) => {
                changed = true;
                details.push(format!("- dal_map[{krate}]: {}", value_label(la)));
            }
            (None, Some(lb)) => {
                changed = true;
                details.push(format!("+ dal_map[{krate}]: {}", value_label(lb)));
            }
            (None, None) => {}
        }
    }

    // `boundary_policy` flag-wise.
    for flag in [
        "no_out_of_scope_deps",
        "forbid_build_rs",
        "forbid_proc_macros",
    ] {
        let fa = va
            .get("boundary_policy")
            .and_then(|p| p.get(flag))
            .cloned()
            .unwrap_or(Value::Null);
        let fb = vb
            .get("boundary_policy")
            .and_then(|p| p.get(flag))
            .cloned()
            .unwrap_or(Value::Null);
        if fa != fb {
            changed = true;
            details.push(format!(
                "~ boundary_policy.{flag}: {} -> {}",
                value_label(&fa),
                value_label(&fb)
            ));
        }
    }

    CategoryDiff {
        category: "scope",
        status: if changed {
            DiffCategoryStatus::Changed
        } else {
            DiffCategoryStatus::Equal
        },
        details,
    }
}

/// One trace file's entries keyed for comparison: uid (or the
/// human id when no uid is recorded) → (human id, entry JSON).
type TraceEntries = BTreeMap<String, (String, Value)>;

/// Parse `trace/<file>` into comparison form. The entry list lives
/// under `requirements` (hlr / llr / derived) or `tests`
/// (tests.toml); entries are compared as raw JSON so the comparison
/// survives schema additions without a type change here.
fn read_trace_entries(side: &Side, file: &str) -> Result<Load<TraceEntries>, DiffError> {
    let path = side.root.join("trace").join(file);
    if !path.exists() {
        return Ok(Load::Missing);
    }
    let content = std::fs::read_to_string(&path).map_err(|source| DiffError::Io {
        path: path.clone(),
        source,
    })?;
    let Ok(value) = toml::from_str::<toml::Value>(&content) else {
        return Ok(Load::Unparseable);
    };
    let mut entries = TraceEntries::new();
    for key in ["requirements", "tests"] {
        if let Some(list) = value.get(key).and_then(toml::Value::as_array) {
            for entry in list {
                let id = entry
                    .get("id")
                    .and_then(toml::Value::as_str)
                    .unwrap_or("(no id)")
                    .to_string();
                let uid = entry
                    .get("uid")
                    .and_then(toml::Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_else(|| format!("id:{id}"));
                let json = serde_json::to_value(entry).unwrap_or(Value::Null);
                entries.insert(uid, (id, json));
            }
        }
    }
    Ok(Load::Ok(entries))
}

/// `trace_graph` — entry-level comparison of every `trace/*.toml`
/// file by requirement uid, plus the generated matrix's presence.
/// A missing `trace/` directory makes the category unverifiable
/// (the graph claim cannot be examined), never silently equal.
pub(crate) fn trace_graph(a: &Side, b: &Side) -> Result<CategoryDiff, DiffError> {
    let dir_a = a.root.join("trace").is_dir();
    let dir_b = b.root.join("trace").is_dir();
    match (dir_a, dir_b) {
        (false, false) => return Ok(unverifiable("trace_graph", "trace/ absent in both bundles")),
        (false, true) => return Ok(unverifiable("trace_graph", "trace/ absent in bundle A")),
        (true, false) => return Ok(unverifiable("trace_graph", "trace/ absent in bundle B")),
        (true, true) => {}
    }

    let mut details = Vec::new();
    let mut changed = false;
    let mut unexaminable = false;
    for file in ["hlr.toml", "llr.toml", "tests.toml", "derived.toml"] {
        let ea = read_trace_entries(a, file)?;
        let eb = read_trace_entries(b, file)?;
        match (&ea, &eb) {
            (Load::Unparseable, _) => {
                unexaminable = true;
                details.push(format!("! trace/{file} unparseable in bundle A"));
            }
            (_, Load::Unparseable) => {
                unexaminable = true;
                details.push(format!("! trace/{file} unparseable in bundle B"));
            }
            (Load::Missing, Load::Missing) => {}
            (Load::Missing, Load::Ok(map)) => {
                changed = true;
                details.push(format!("! trace/{file} absent in bundle A"));
                for (id, _) in map.values() {
                    details.push(format!("+ {file}: {id}"));
                }
            }
            (Load::Ok(map), Load::Missing) => {
                changed = true;
                details.push(format!("! trace/{file} absent in bundle B"));
                for (id, _) in map.values() {
                    details.push(format!("- {file}: {id}"));
                }
            }
            (Load::Ok(ma), Load::Ok(mb)) => {
                for (uid, (id, ja)) in ma {
                    match mb.get(uid) {
                        None => {
                            changed = true;
                            details.push(format!("- {file}: {id}"));
                        }
                        Some((_, jb)) if jb != ja => {
                            changed = true;
                            details.push(format!("~ {file}: {id}"));
                        }
                        Some(_) => {}
                    }
                }
                for (uid, (id, _)) in mb {
                    if !ma.contains_key(uid) {
                        changed = true;
                        details.push(format!("+ {file}: {id}"));
                    }
                }
            }
        }
    }

    // The generated matrix is content-derived; only its presence is
    // compared.
    let matrix_a = file_exists(a, "trace/matrix.md");
    let matrix_b = file_exists(b, "trace/matrix.md");
    if matrix_a != matrix_b {
        changed = true;
        let (la, lb) = (presence_label(matrix_a), presence_label(matrix_b));
        details.push(format!("~ trace/matrix.md: {la} -> {lb}"));
    }

    Ok(CategoryDiff {
        category: "trace_graph",
        status: status_of(changed, unexaminable),
        details,
    })
}

/// `recipe` — the canonical recipe fields of
/// `deterministic-manifest.json`, reusing the reproduction
/// comparison's field set and loaders (no duplicated plane logic).
pub(crate) fn recipe(a: &Side, b: &Side) -> Result<CategoryDiff, DiffError> {
    let ra = read_recipe(&a.root).map_err(map_reproduction_error)?;
    let rb = read_recipe(&b.root).map_err(map_reproduction_error)?;
    let (Some(ra), Some(rb)) = (&ra, &rb) else {
        let side = match (ra.is_none(), rb.is_none()) {
            (true, true) => "both bundles",
            (true, false) => "bundle A",
            (false, true) => "bundle B",
            (false, false) => "neither bundle",
        };
        return Ok(unverifiable(
            "recipe",
            format!("deterministic-manifest.json missing or unparseable in {side}"),
        ));
    };
    let details: Vec<String> = compare_recipe_fields(ra, rb)
        .into_iter()
        .map(|field| format!("~ {field}"))
        .collect();
    Ok(CategoryDiff {
        category: "recipe",
        status: if details.is_empty() {
            DiffCategoryStatus::Equal
        } else {
            DiffCategoryStatus::Changed
        },
        details,
    })
}

/// `inputs` / `outputs` — the digest planes, reusing the
/// reproduction comparison's plane loader and directional key diff.
pub(crate) fn digest_plane(
    a: &Side,
    b: &Side,
    category: &'static str,
) -> Result<CategoryDiff, DiffError> {
    let file = format!("{category}_hashes.json");
    let pa = read_digest_map(&a.root, &file).map_err(map_reproduction_error)?;
    let pb = read_digest_map(&b.root, &file).map_err(map_reproduction_error)?;
    let (Plane::Loaded(ma), Plane::Loaded(mb)) = (&pa, &pb) else {
        let side = match (
            matches!(pa, Plane::Unavailable),
            matches!(pb, Plane::Unavailable),
        ) {
            (true, true) => "both bundles",
            (true, false) => "bundle A",
            (false, true) => "bundle B",
            (false, false) => "neither bundle",
        };
        return Ok(unverifiable(
            category,
            format!("{file} missing or unparseable in {side}"),
        ));
    };
    let diff = diff_digest_planes(ma, mb);
    let mut details = Vec::new();
    for path in &diff.unverifiable {
        details.push(format!("! {path} (malformed digest)"));
    }
    for path in &diff.cand_only {
        details.push(format!("+ {path}"));
    }
    for path in &diff.base_only {
        details.push(format!("- {path}"));
    }
    for path in &diff.changed {
        details.push(format!("~ {path}"));
    }
    Ok(CategoryDiff {
        category,
        status: if details.is_empty() {
            DiffCategoryStatus::Equal
        } else {
            DiffCategoryStatus::Changed
        },
        details,
    })
}

/// Map the reproduction loader's error family onto ours. The
/// `BundleNotFound` arm is unreachable from per-file loaders (the
/// facade already checked both roots), mapped for exhaustiveness.
fn map_reproduction_error(e: crate::verify::ReproductionError) -> DiffError {
    match e {
        crate::verify::ReproductionError::Io { path, source } => DiffError::Io { path, source },
        crate::verify::ReproductionError::BundleNotFound(path) => DiffError::BundleNotFound(path),
    }
}

/// Presence label used in `present -> absent`-style details.
pub(crate) fn presence_label(present: bool) -> &'static str {
    if present { "present" } else { "absent" }
}

/// Status precedence shared by the multi-artifact categories: real
/// differences outrank unexaminable parts (which still appear as
/// `!`-prefixed details); unexaminable parts outrank a clean Equal.
pub(crate) fn status_of(changed: bool, unexaminable: bool) -> DiffCategoryStatus {
    if changed {
        DiffCategoryStatus::Changed
    } else if unexaminable {
        DiffCategoryStatus::Unverifiable
    } else {
        DiffCategoryStatus::Equal
    }
}
