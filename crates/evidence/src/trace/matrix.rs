//! Markdown traceability matrix generation.
//!
//! Output is fully deterministic (no timestamps, `BTreeSet`/sorted
//! vectors everywhere) so regenerating the matrix twice on the same
//! inputs yields byte-identical bytes — a hard requirement for
//! inclusion in hashed evidence bundles.
//!
//! The document contains four tables: HLR→LLR, LLR→Test,
//! reverse Test→LLR→HLR, and an end-to-end HLR→Test roll-up, plus
//! an annotations block for DO-178C metadata (scope, category,
//! source, modules, test_selector) that doesn't fit the columnar
//! tables, plus a coverage + gaps summary.

use anyhow::Result;
use std::collections::{BTreeMap, BTreeSet};

use super::entries::{HlrFile, LlrEntry, LlrFile, TestEntry, TestsFile};

/// Generate a Markdown traceability matrix document.
///
/// The output is deterministic (no timestamps) for reproducibility.
pub fn generate_traceability_matrix(
    hlr: &HlrFile,
    llr: &LlrFile,
    tests: &TestsFile,
    doc_id: &str,
) -> Result<String> {
    let mut s = String::new();

    // Header — NO TIMESTAMPS for determinism.
    s.push_str("<!-- GENERATED FILE. DO NOT EDIT.\n");
    s.push_str("     Source of truth: cert/trace/*.toml\n");
    s.push_str("     Regenerate: cargo xtask trace\n");
    s.push_str("-->\n\n");
    s.push_str("# Traceability Matrix\n\n");
    s.push_str("<!-- Source: cert/trace roots (see project.toml trace.roots) -->\n");
    s.push_str(&format!("**Document ID:** {}\n\n", doc_id));

    s.push_str("## Schema & Provenance\n\n");
    s.push_str(&format!(
        "- **HLR:** schema={}, document={}, rev={}\n",
        hlr.schema.version, hlr.meta.document_id, hlr.meta.revision
    ));
    s.push_str(&format!(
        "- **LLR:** schema={}, document={}, rev={}\n",
        llr.schema.version, llr.meta.document_id, llr.meta.revision
    ));
    s.push_str(&format!(
        "- **Tests:** schema={}, document={}, rev={}\n\n",
        tests.schema.version, tests.meta.document_id, tests.meta.revision
    ));

    // Sort by sort_key, then by ID for determinism.
    let mut hlrs = hlr.requirements.clone();
    hlrs.sort_by(|a, b| {
        a.sort_key
            .unwrap_or(0)
            .cmp(&b.sort_key.unwrap_or(0))
            .then_with(|| a.id.cmp(&b.id))
    });

    let mut llrs = llr.requirements.clone();
    llrs.sort_by(|a, b| {
        a.sort_key
            .unwrap_or(0)
            .cmp(&b.sort_key.unwrap_or(0))
            .then_with(|| a.id.cmp(&b.id))
    });

    let mut ts = tests.tests.clone();
    ts.sort_by(|a, b| {
        a.sort_key
            .unwrap_or(0)
            .cmp(&b.sort_key.unwrap_or(0))
            .then_with(|| a.id.cmp(&b.id))
    });

    // HLR -> LLR table.
    s.push_str("## HLR to LLR Traceability\n\n");
    s.push_str("| HLR ID | HLR Title | LLR IDs |\n");
    s.push_str("|--------|-----------|--------|\n");

    for h in &hlrs {
        // LLRs that trace to this HLR (strict UUID match).
        let mut linked: Vec<&str> = llrs
            .iter()
            .filter(|l| l.traces_to.iter().any(|x| Some(x) == h.uid.as_ref()))
            .map(|l| l.id.as_str())
            .collect();
        linked.sort();

        let llr_cell = if linked.is_empty() {
            "*(none)*".to_string()
        } else {
            linked.join(", ")
        };
        let display_id = if let Some(ns) = &h.ns {
            format!("{}:{}", ns, h.id)
        } else {
            h.id.clone()
        };
        s.push_str(&format!(
            "| {} | {} | {} |\n",
            display_id, h.title, llr_cell
        ));
    }

    // LLR -> TEST table.
    s.push_str("\n## LLR to Test Traceability\n\n");
    s.push_str("| LLR ID | LLR Title | Test IDs |\n");
    s.push_str("|--------|-----------|----------|\n");

    for l in &llrs {
        let mut linked: Vec<&str> = ts
            .iter()
            .filter(|t| t.traces_to.iter().any(|x| Some(x) == l.uid.as_ref()))
            .map(|t| t.id.as_str())
            .collect();
        linked.sort();

        let test_cell = if linked.is_empty() {
            "*(none)*".to_string()
        } else {
            linked.join(", ")
        };
        let display_id = if let Some(ns) = &l.ns {
            format!("{}:{}", ns, l.id)
        } else {
            l.id.clone()
        };
        s.push_str(&format!(
            "| {} | {} | {} |\n",
            display_id, l.title, test_cell
        ));
    }

    // Reverse trace: Test -> LLR -> HLR.
    // Build lookup: LLR UID -> list of HLR IDs it traces to.
    let mut llr_uid_to_hlr_ids: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for l in &llrs {
        if let Some(ref uid) = l.uid {
            let mut parent_ids: Vec<String> = Vec::new();
            for link in &l.traces_to {
                if let Some(h) = hlrs.iter().find(|h| h.uid.as_ref() == Some(link)) {
                    let display_id = if let Some(ns) = &h.ns {
                        format!("{}:{}", ns, h.id)
                    } else {
                        h.id.clone()
                    };
                    parent_ids.push(display_id);
                }
            }
            parent_ids.sort();
            llr_uid_to_hlr_ids.insert(uid.clone(), parent_ids);
        }
    }

    s.push_str("\n## Reverse Trace: Test to LLR to HLR\n\n");
    s.push_str("| Test ID | LLR IDs | HLR IDs |\n");
    s.push_str("|---------|---------|--------|\n");

    for t in &ts {
        // LLR IDs this test traces to.
        let mut llr_ids: Vec<String> = Vec::new();
        let mut hlr_ids_set: BTreeSet<String> = BTreeSet::new();

        for link in &t.traces_to {
            if let Some(l) = llrs.iter().find(|l| l.uid.as_ref() == Some(link)) {
                let display_id = if let Some(ns) = &l.ns {
                    format!("{}:{}", ns, l.id)
                } else {
                    l.id.clone()
                };
                llr_ids.push(display_id);

                // Roll up to HLR via LLR UID.
                if let Some(ref uid) = l.uid {
                    if let Some(parent_hlrs) = llr_uid_to_hlr_ids.get(uid) {
                        for h_id in parent_hlrs {
                            hlr_ids_set.insert(h_id.clone());
                        }
                    }
                }
            }
        }
        llr_ids.sort();

        let llr_cell = if llr_ids.is_empty() {
            "*(none)*".to_string()
        } else {
            llr_ids.join(", ")
        };
        let hlr_cell = if hlr_ids_set.is_empty() {
            "*(none)*".to_string()
        } else {
            hlr_ids_set.into_iter().collect::<Vec<_>>().join(", ")
        };

        let display_id = if let Some(ns) = &t.ns {
            format!("{}:{}", ns, t.id)
        } else {
            t.id.clone()
        };
        s.push_str(&format!(
            "| {} | {} | {} |\n",
            display_id, llr_cell, hlr_cell
        ));
    }

    // Annotations: scope/category/source/modules/test_selector.
    // (Preserves DO-178C metadata that's not shown in the tables above.)
    let mut annotations = String::new();
    for h in &hlrs {
        let mut parts: Vec<String> = Vec::new();
        if let Some(ref v) = h.scope {
            parts.push(format!("scope={}", v));
        }
        if let Some(ref v) = h.category {
            parts.push(format!("category={}", v));
        }
        if let Some(ref v) = h.source {
            parts.push(format!("source={}", v));
        }
        if !parts.is_empty() {
            annotations.push_str(&format!("- HLR {}: {}\n", h.id, parts.join(", ")));
        }
    }
    for l in &llrs {
        let mut parts: Vec<String> = Vec::new();
        if let Some(ref v) = l.source {
            parts.push(format!("source={}", v));
        }
        if !l.modules.is_empty() {
            parts.push(format!("modules=[{}]", l.modules.join(", ")));
        }
        if !parts.is_empty() {
            annotations.push_str(&format!("- LLR {}: {}\n", l.id, parts.join(", ")));
        }
    }
    for t in &ts {
        let mut parts: Vec<String> = Vec::new();
        if let Some(ref v) = t.category {
            parts.push(format!("category={}", v));
        }
        if let Some(ref v) = t.test_selector {
            parts.push(format!("selector={}", v));
        }
        if let Some(ref v) = t.source {
            parts.push(format!("source={}", v));
        }
        if !parts.is_empty() {
            annotations.push_str(&format!("- TEST {}: {}\n", t.id, parts.join(", ")));
        }
    }
    if !annotations.is_empty() {
        s.push_str("\n## Annotations\n\n");
        s.push_str(&annotations);
    }

    // End-to-end HLR -> Test roll-up table.
    s.push_str("\n## End-to-End: HLR to Test Roll-Up\n\n");
    s.push_str("| HLR ID | HLR Title | Test IDs (via LLR) |\n");
    s.push_str("|--------|-----------|--------------------|\n");

    for h in &hlrs {
        // All LLRs that trace to this HLR.
        let child_llrs: Vec<&LlrEntry> = llrs
            .iter()
            .filter(|l| l.traces_to.iter().any(|x| Some(x) == h.uid.as_ref()))
            .collect();

        // All tests that trace to any of those LLRs.
        let mut test_ids: BTreeSet<String> = BTreeSet::new();
        for l in &child_llrs {
            for t in &ts {
                if t.traces_to.iter().any(|x| l.uid.as_ref() == Some(x)) {
                    let display_id = if let Some(ns) = &t.ns {
                        format!("{}:{}", ns, t.id)
                    } else {
                        t.id.clone()
                    };
                    test_ids.insert(display_id);
                }
            }
        }

        let test_cell = if test_ids.is_empty() {
            "*(none)*".to_string()
        } else {
            test_ids.into_iter().collect::<Vec<_>>().join(", ")
        };
        let display_id = if let Some(ns) = &h.ns {
            format!("{}:{}", ns, h.id)
        } else {
            h.id.clone()
        };
        s.push_str(&format!(
            "| {} | {} | {} |\n",
            display_id, h.title, test_cell
        ));
    }

    // Orphan test detection + coverage + gaps.
    let orphan_tests: Vec<&TestEntry> = ts.iter().filter(|t| t.traces_to.is_empty()).collect();

    let hlr_without_llr: usize = hlrs
        .iter()
        .filter(|h| {
            !llrs
                .iter()
                .any(|l| l.traces_to.iter().any(|x| Some(x) == h.uid.as_ref()))
        })
        .count();
    let llr_without_test: usize = llrs
        .iter()
        .filter(|l| {
            !ts.iter()
                .any(|t| t.traces_to.iter().any(|x| Some(x) == l.uid.as_ref()))
        })
        .count();

    s.push_str("\n## Coverage Summary\n\n");
    s.push_str(&format!("- **HLR count:** {}\n", hlrs.len()));
    s.push_str(&format!("- **LLR count:** {}\n", llrs.len()));
    s.push_str(&format!("- **Test count:** {}\n", ts.len()));
    s.push_str(&format!("- **HLR without LLR:** {}\n", hlr_without_llr));
    s.push_str(&format!("- **LLR without Test:** {}\n", llr_without_test));
    s.push_str(&format!(
        "- **Orphan tests (no LLR link):** {}\n",
        orphan_tests.len()
    ));
    s.push('\n');

    if hlr_without_llr > 0 || llr_without_test > 0 || !orphan_tests.is_empty() {
        s.push_str("## Gaps\n\n");

        if hlr_without_llr > 0 {
            s.push_str("### HLRs without LLR coverage\n\n");
            for h in &hlrs {
                if !llrs
                    .iter()
                    .any(|l| l.traces_to.iter().any(|x| Some(x) == h.uid.as_ref()))
                {
                    s.push_str(&format!("- {} ({})\n", h.id, h.title));
                }
            }
            s.push('\n');
        }

        if llr_without_test > 0 {
            s.push_str("### LLRs without Test coverage\n\n");
            for l in &llrs {
                if !ts
                    .iter()
                    .any(|t| t.traces_to.iter().any(|x| Some(x) == l.uid.as_ref()))
                {
                    s.push_str(&format!("- {} ({})\n", l.id, l.title));
                }
            }
            s.push('\n');
        }

        if !orphan_tests.is_empty() {
            s.push_str("### Orphan Tests (no LLR link)\n\n");
            for t in &orphan_tests {
                let display_id = if let Some(ns) = &t.ns {
                    format!("{}:{}", ns, t.id)
                } else {
                    t.id.clone()
                };
                s.push_str(&format!("- {} ({})\n", display_id, t.title));
            }
            s.push('\n');
        }
    }

    Ok(s)
}
