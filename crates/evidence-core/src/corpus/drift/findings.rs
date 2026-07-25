//! Typed drift findings, the closed category set, and the
//! canonical report rendering (LLR-177, LLR-178).
//!
//! A [`DriftFinding`] is one typed observation of one plane of a
//! re-ingestion comparison: what moved (the closed
//! [`DriftCategory`]), where (the node or patch uid and the
//! structural path), and the category-specific typed detail
//! ([`DriftDetail`]). Findings never carry timestamps, absolute
//! paths, map or file layout, so equivalent inputs render
//! identically.
//!
//! Findings sort by category (declaration order), structural path,
//! and uid; the report-level sort key prefixes the source document
//! and source revision, so an aggregated multi-revision listing
//! stays deterministic. Zero findings is explicit equality:
//! [`DriftReport::outcome`] reports [`DriftOutcome::Equal`] and the
//! canonical rendering pins `outcome = equal` with the
//! `output_equal` category name — the equality marker of the
//! closed category set, never a finding.
//!
//! [`render_report_canonical`] byte-locks the report under the
//! domain/version tag `evidence/reingestion-drift/v1` with the
//! minimal TOML escaping of the canonical source-graph rendering;
//! the golden fixtures pin the exact bytes.

use std::fmt;

/// The closed set of drift categories (LLR-177, LLR-178). `Ord` is
/// the declaration order, used only as the deterministic sort rank.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DriftCategory {
    /// The candidate recipe identity moved, or none was presented.
    RecipeChangedOrUnavailable,
    /// The candidate verified-input digest moved, or none was
    /// presented (missing or unverifiable upstream).
    VerifiedInputChanged,
    /// The candidate extractor-output digest moved, or none was
    /// presented, against a baseline that carries one (PDF only;
    /// LLR-183).
    ExtractorOutputChanged,
    /// A candidate node matched no committed identity.
    NodeAdded,
    /// A committed node matched no candidate.
    NodeRemoved,
    /// A candidate matched a structural key claimed by more than
    /// one committed node, so the pairing is formally ambiguous.
    NodeUnreconciled,
    /// A matched node's closed structural kind changed.
    NodeKindChanged,
    /// A matched node's parent linkage changed.
    NodeParentChanged,
    /// A matched node's sibling ordinal changed.
    NodeOrdinalChanged,
    /// A matched node's label changed.
    NodeLabelChanged,
    /// A matched node's canonical text changed.
    NodeCanonicalTextChanged,
    /// A matched node's content digest changed.
    NodeContentDigestChanged,
    /// A matched node's structural fingerprint changed.
    NodeStructuralFingerprintChanged,
    /// A matched node's semantic locator fields (variant, path or
    /// canonical URL, anchor or fragment, heading path) changed.
    NodeSemanticLocatorChanged,
    /// A matched node's diagnostic-only positions (byte range, DOM
    /// path, page, bounding box, printed label, final URL, git
    /// blob) moved. Never semantic drift.
    DiagnosticLocatorMoved,
    /// A candidate-plane patch has no committed patch with its uid.
    PatchAdded,
    /// A committed patch has no candidate-plane patch with its uid.
    PatchRemoved,
    /// A patch's reviewed-content digest moved between the planes.
    PatchChanged,
    /// A patch's candidate-plane lifecycle evaluation is `Stale`.
    PatchStale,
    /// A patch's candidate-plane lifecycle evaluation is
    /// `Rejected`.
    PatchRejected,
    /// A patch record is malformed (its reviewed-content digest
    /// does not recompute, or its uid repeats in the candidate
    /// plane) or its approved application against the candidate
    /// graph fails.
    PatchUnappliable,
    /// A patch's evaluated lifecycle state differs between the
    /// committed and candidate planes.
    ReviewStateChanged,
    /// The approval-gated effective graph digest moved.
    EffectiveGraphChanged,
    /// The equality marker: every plane compared equal. Carried by
    /// [`DriftOutcome::Equal`], never by a finding.
    OutputEqual,
}

impl DriftCategory {
    /// The category's snake_case wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            DriftCategory::RecipeChangedOrUnavailable => "recipe_changed_or_unavailable",
            DriftCategory::VerifiedInputChanged => "verified_input_changed",
            DriftCategory::ExtractorOutputChanged => "extractor_output_changed",
            DriftCategory::NodeAdded => "node_added",
            DriftCategory::NodeRemoved => "node_removed",
            DriftCategory::NodeUnreconciled => "node_unreconciled",
            DriftCategory::NodeKindChanged => "node_kind_changed",
            DriftCategory::NodeParentChanged => "node_parent_changed",
            DriftCategory::NodeOrdinalChanged => "node_ordinal_changed",
            DriftCategory::NodeLabelChanged => "node_label_changed",
            DriftCategory::NodeCanonicalTextChanged => "node_canonical_text_changed",
            DriftCategory::NodeContentDigestChanged => "node_content_digest_changed",
            DriftCategory::NodeStructuralFingerprintChanged => {
                "node_structural_fingerprint_changed"
            }
            DriftCategory::NodeSemanticLocatorChanged => "node_semantic_locator_changed",
            DriftCategory::DiagnosticLocatorMoved => "diagnostic_locator_moved",
            DriftCategory::PatchAdded => "patch_added",
            DriftCategory::PatchRemoved => "patch_removed",
            DriftCategory::PatchChanged => "patch_changed",
            DriftCategory::PatchStale => "patch_stale",
            DriftCategory::PatchRejected => "patch_rejected",
            DriftCategory::PatchUnappliable => "patch_unappliable",
            DriftCategory::ReviewStateChanged => "review_state_changed",
            DriftCategory::EffectiveGraphChanged => "effective_graph_changed",
            DriftCategory::OutputEqual => "output_equal",
        }
    }
}

impl fmt::Display for DriftCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The category-specific typed context of one finding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriftDetail {
    /// A plane identity digest moved; `None` candidate means the
    /// candidate identity was unavailable.
    PlaneDigest {
        /// The baseline digest, hex.
        baseline: String,
        /// The candidate digest, hex, when presented.
        candidate: Option<String>,
    },
    /// One named field changed; values render deterministically.
    FieldChange {
        /// The changed field's wire name.
        field: &'static str,
        /// The committed value, rendered.
        committed: String,
        /// The candidate value, rendered.
        candidate: String,
    },
    /// A candidate-only node or patch; the uid is on the finding.
    CandidateOnly,
    /// A committed-only node or patch; the uid is on the finding.
    CommittedOnly,
    /// An ambiguous structural-key pool: every committed uid
    /// sharing the key, in uid order.
    AmbiguousKey {
        /// The committed uids sharing the structural key.
        committed_pool: Vec<String>,
    },
    /// A patch reviewed-content digest moved between the planes.
    PatchDigest {
        /// The committed digest, hex.
        committed: String,
        /// The candidate digest, hex.
        candidate: String,
    },
    /// The evaluated lifecycle states of the two planes.
    LifecycleStates {
        /// The committed-plane state.
        committed: String,
        /// The candidate-plane state.
        candidate: String,
    },
    /// A failure rendered from its typed error, never stringified
    /// structure.
    Failure {
        /// The rendered failure.
        message: String,
    },
    /// The effective graph digests of the two planes.
    EffectiveDigests {
        /// The committed effective digest, hex.
        committed: String,
        /// The candidate effective digest, hex.
        candidate: String,
    },
}

/// One typed drift finding (LLR-177, LLR-178).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriftFinding {
    /// The closed category.
    pub category: DriftCategory,
    /// The node's deterministic structural path, when the finding
    /// names a node: label or `kind[ordinal]` segments, root first.
    pub structural_path: Option<String>,
    /// The node uid the finding names: the committed uid for
    /// matched and removed nodes, the candidate uid for added and
    /// unreconciled nodes.
    pub node_uid: Option<String>,
    /// The patch uid the finding names.
    pub patch_uid: Option<String>,
    /// The category-specific typed detail.
    pub detail: DriftDetail,
}

impl DriftFinding {
    /// The deterministic within-report sort key: category, then
    /// structural path, then node uid, then patch uid.
    pub(crate) fn sort_key(&self) -> (DriftCategory, Option<&str>, Option<&str>, Option<&str>) {
        (
            self.category,
            self.structural_path.as_deref(),
            self.node_uid.as_deref(),
            self.patch_uid.as_deref(),
        )
    }
}

/// The comparison outcome: explicit equality or drift.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriftOutcome {
    /// Every plane compared equal; the report carries no findings.
    Equal,
    /// At least one finding was emitted.
    Drifted,
}

impl DriftOutcome {
    /// The outcome's wire name.
    fn as_str(self) -> &'static str {
        match self {
            DriftOutcome::Equal => "equal",
            DriftOutcome::Drifted => "drifted",
        }
    }
}

/// The deterministic, read-only result of one re-ingestion
/// comparison (LLR-178).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriftReport {
    /// The candidate-presented source-document identity (canonical
    /// path or URL); the first report sort key.
    pub source_document: String,
    /// The compared source revision.
    pub source_revision_uid: String,
    /// The typed findings, sorted by category, structural path,
    /// and uid. Empty under explicit equality.
    pub findings: Vec<DriftFinding>,
}

impl DriftReport {
    /// The outcome: [`DriftOutcome::Equal`] exactly when no
    /// findings were emitted.
    pub fn outcome(&self) -> DriftOutcome {
        if self.findings.is_empty() {
            DriftOutcome::Equal
        } else {
            DriftOutcome::Drifted
        }
    }

    /// Whether every plane compared equal.
    pub fn is_equal(&self) -> bool {
        self.outcome() == DriftOutcome::Equal
    }
}

/// Domain/version tag opening the canonical report rendering.
const REPORT_HEADER: &str = "evidence/reingestion-drift/v1";

/// Render `report` into the canonical byte form pinned by the
/// module docs and byte-locked by the golden fixtures. Pure and
/// host-independent.
pub fn render_report_canonical(report: &DriftReport) -> Vec<u8> {
    let mut out = String::new();
    out.push_str(REPORT_HEADER);
    out.push('\n');
    push_field(&mut out, "source_document", &report.source_document);
    push_field(&mut out, "source_revision_uid", &report.source_revision_uid);
    out.push_str(&format!("outcome = {}\n", report.outcome().as_str()));
    if report.is_equal() {
        push_field(&mut out, "category", DriftCategory::OutputEqual.as_str());
    }
    out.push_str(&format!("findings = {}\n", report.findings.len()));
    for finding in &report.findings {
        out.push_str("\n[[finding]]\n");
        push_field(&mut out, "category", finding.category.as_str());
        if let Some(path) = &finding.structural_path {
            push_field(&mut out, "structural_path", path);
        }
        if let Some(uid) = &finding.node_uid {
            push_field(&mut out, "node_uid", uid);
        }
        if let Some(uid) = &finding.patch_uid {
            push_field(&mut out, "patch_uid", uid);
        }
        push_detail(&mut out, &finding.detail);
    }
    out.into_bytes()
}

/// Render one detail block in declaration order.
fn push_detail(out: &mut String, detail: &DriftDetail) {
    match detail {
        DriftDetail::PlaneDigest {
            baseline,
            candidate,
        } => {
            push_field(out, "baseline_digest", baseline);
            match candidate {
                Some(candidate) => push_field(out, "candidate_digest", candidate),
                None => push_field(out, "candidate_digest", "<unavailable>"),
            }
        }
        DriftDetail::FieldChange {
            field,
            committed,
            candidate,
        } => {
            push_field(out, "field", field);
            push_field(out, "committed", committed);
            push_field(out, "candidate", candidate);
        }
        DriftDetail::CandidateOnly => push_field(out, "presence", "candidate_only"),
        DriftDetail::CommittedOnly => push_field(out, "presence", "committed_only"),
        DriftDetail::AmbiguousKey { committed_pool } => {
            out.push_str("committed_pool = [");
            for (index, uid) in committed_pool.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                push_basic_string(out, uid);
            }
            out.push_str("]\n");
        }
        DriftDetail::PatchDigest {
            committed,
            candidate,
        } => {
            push_field(out, "committed_digest", committed);
            push_field(out, "candidate_digest", candidate);
        }
        DriftDetail::LifecycleStates {
            committed,
            candidate,
        } => {
            push_field(out, "committed_state", committed);
            push_field(out, "candidate_state", candidate);
        }
        DriftDetail::Failure { message } => push_field(out, "failure", message),
        DriftDetail::EffectiveDigests {
            committed,
            candidate,
        } => {
            push_field(out, "committed_effective", committed);
            push_field(out, "candidate_effective", candidate);
        }
    }
}

/// One `key = "<value>"` line in canonical escaping.
fn push_field(out: &mut String, key: &str, value: &str) {
    out.push_str(key);
    out.push_str(" = ");
    push_basic_string(out, value);
    out.push('\n');
}

/// Append `value` as a TOML basic string with deterministic minimal
/// escaping — the same rules the canonical source-graph rendering
/// pins.
fn push_basic_string(out: &mut String, value: &str) {
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{08}' => out.push_str("\\b"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\u{0C}' => out.push_str("\\f"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x20 || (c as u32) == 0x7F => {
                out.push_str(&format!("\\u{:04X}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}
