//! Shared semantic evaluation of the trace evidence behind a
//! requested assurance claim (LLR-105 / HLR-086 / SYS-036).
//!
//! Absence of evidence is an adoption state, not valid evidence.
//! A missing trace root or an all-empty trace tree must not
//! terminate in `VERIFY_OK`: the link validator passes vacuously
//! on zero requirements, so any surface that only ran the
//! validator would inherit the silent pass.
//!
//! [`evaluate_trace_evidence`] is THE semantic result every
//! surface consumes — `trace --validate`, `check`, doctor's
//! trace-validity check, `generate`'s phase 6, and (via the same
//! two semantic codes) bundle verification. It classifies into
//! five states and only [`TraceEvidenceState::Valid`] satisfies a
//! claim.

use std::path::Path;

use super::read::{TraceReadError, read_all_trace_files};
use super::validation::{TraceValidationError, validate_trace_links_with_policy};
use crate::policy::TracePolicy;

/// Semantic state of the trace evidence behind a requested
/// assurance claim.
///
/// The first three states carry no evidence at all; the requested
/// claim cannot be satisfied and every consumer must fail closed
/// (or, on development-mode surfaces, report the explicit
/// non-success adoption diagnostic named by
/// [`TraceEvidenceState::gap_code`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TraceEvidenceState {
    /// No trace roots were configured or discoverable.
    NotConfigured,
    /// Trace roots were configured but every one is missing on
    /// disk — the project has not adopted trace evidence yet.
    NotAdopted {
        /// The configured roots that do not exist.
        missing_roots: Vec<String>,
    },
    /// At least one root exists, but the requirements across the
    /// SYS, HLR, LLR, TEST, and DERIVED layers total zero.
    Empty,
    /// Evidence exists but failed to read, parse, or validate.
    /// The underlying error is carried separately on
    /// [`TraceEvidenceEval::validation`] /
    /// [`TraceEvidenceEval::read_error`].
    Invalid,
    /// Non-empty evidence that passed validation — the only state
    /// that satisfies an assurance claim.
    Valid,
}

impl TraceEvidenceState {
    /// `true` iff this state satisfies a requested assurance claim.
    /// Only [`TraceEvidenceState::Valid`] does — every other state
    /// is an adoption state or a validation failure, never proof.
    pub fn satisfies_claim(&self) -> bool {
        matches!(self, Self::Valid)
    }

    /// Stable diagnostic code for the three no-evidence states, or
    /// `None` for `Invalid` (whose consumers emit the carried
    /// validation error's own per-variant codes) and `Valid` (no
    /// gap to diagnose). The strings are the same semantic codes
    /// bundle verification emits from its typed `VerifyError`
    /// variants, so every surface names a no-evidence state
    /// identically.
    pub fn gap_code(&self) -> Option<&'static str> {
        match self {
            Self::NotConfigured => Some("TRACE_EVIDENCE_NOT_CONFIGURED"),
            Self::NotAdopted { .. } => Some("TRACE_EVIDENCE_NOT_ADOPTED"),
            Self::Empty => Some("TRACE_EVIDENCE_EMPTY"),
            Self::Invalid | Self::Valid => None,
        }
    }
}

/// The full evaluation of a set of trace roots: the classified
/// [`TraceEvidenceState`] plus the evidence the classification was
/// derived from, so consumers can render precise messages without
/// re-walking the roots.
#[derive(Debug)]
pub struct TraceEvidenceEval {
    /// The classified semantic state.
    pub state: TraceEvidenceState,
    /// Every root the evaluation was asked about, in caller order.
    pub roots: Vec<String>,
    /// The subset of `roots` that does not exist on disk.
    pub missing_roots: Vec<String>,
    /// Total requirements read across the SYS, HLR, LLR, TEST, and
    /// DERIVED layers of every present root.
    pub requirement_count: usize,
    /// The link-validation failure, when the state is
    /// [`TraceEvidenceState::Invalid`] due to validation. Consumers
    /// iterate `Link` errors for per-variant diagnostics.
    pub validation: Option<TraceValidationError>,
    /// The first read/parse failure, when the state is
    /// [`TraceEvidenceState::Invalid`] because a root could not be
    /// loaded at all. Rendered as the error's own `Display`.
    pub read_error: Option<TraceReadError>,
}

impl TraceEvidenceEval {
    /// `true` iff the evaluated evidence satisfies a requested
    /// assurance claim — convenience forwarder to
    /// [`TraceEvidenceState::satisfies_claim`].
    pub fn satisfies_claim(&self) -> bool {
        self.state.satisfies_claim()
    }
}

/// Evaluate the trace evidence behind a requested assurance claim.
///
/// Classification, in decision order:
///
/// 1. `roots` empty → [`TraceEvidenceState::NotConfigured`].
/// 2. Every root missing on disk → [`TraceEvidenceState::NotAdopted`].
/// 3. A present root fails to read or parse →
///    [`TraceEvidenceState::Invalid`] with the read error.
/// 4. Zero requirements across every layer →
///    [`TraceEvidenceState::Empty`]. Validation is NOT run on zero
///    requirements — it would pass vacuously, which is exactly the
///    silent pass this module exists to close.
/// 5. Validation failure → [`TraceEvidenceState::Invalid`] with the
///    typed error.
/// 6. Otherwise → [`TraceEvidenceState::Valid`].
///
/// Requirements from every present root are aggregated before
/// counting and validation (matching doctor's historical
/// cross-root semantics). Root list order is non-semantic: the
/// resulting state, count, and missing-root set do not depend on
/// it. `policy` is read, not written.
pub fn evaluate_trace_evidence(roots: &[String], policy: &TracePolicy) -> TraceEvidenceEval {
    let mut eval = TraceEvidenceEval {
        state: TraceEvidenceState::NotConfigured,
        roots: roots.to_vec(),
        missing_roots: Vec::new(),
        requirement_count: 0,
        validation: None,
        read_error: None,
    };
    if roots.is_empty() {
        return eval;
    }

    let mut sys = Vec::new();
    let mut hlr = Vec::new();
    let mut llr = Vec::new();
    let mut tests = Vec::new();
    let mut derived = Vec::new();
    for root in roots {
        if !Path::new(root).exists() {
            eval.missing_roots.push(root.clone());
            continue;
        }
        let files = match read_all_trace_files(root) {
            Ok(f) => f,
            Err(e) => {
                eval.state = TraceEvidenceState::Invalid;
                eval.read_error = Some(e);
                return eval;
            }
        };
        eval.requirement_count += files.sys.requirements.len()
            + files.hlr.requirements.len()
            + files.llr.requirements.len()
            + files.tests.tests.len()
            + files
                .derived
                .as_ref()
                .map(|d| d.requirements.len())
                .unwrap_or(0);
        sys.extend(files.sys.requirements);
        hlr.extend(files.hlr.requirements);
        llr.extend(files.llr.requirements);
        tests.extend(files.tests.tests);
        if let Some(d) = files.derived {
            derived.extend(d.requirements);
        }
    }

    if eval.requirement_count == 0 {
        eval.state = if eval.missing_roots.len() == roots.len() {
            TraceEvidenceState::NotAdopted {
                missing_roots: eval.missing_roots.clone(),
            }
        } else {
            TraceEvidenceState::Empty
        };
        return eval;
    }

    match validate_trace_links_with_policy(&sys, &hlr, &llr, &tests, &derived, policy) {
        Ok(()) => {
            eval.state = TraceEvidenceState::Valid;
        }
        Err(e) => {
            eval.state = TraceEvidenceState::Invalid;
            eval.validation = Some(e);
        }
    }
    eval
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
mod tests {
    use super::*;
    use crate::trace::entries::{HlrEntry, LlrEntry, TestEntry};

    const SCHEMA_V: &str = crate::schema_versions::TRACE;

    /// Write the four trace files into `dir` from pre-built entry
    /// vectors. Serialization via the typed `*File` wrappers keeps
    /// the fixture honest — it parses through the same `read_toml`
    /// path production uses.
    fn seed_root(
        dir: &Path,
        sys: &[HlrEntry],
        hlr: &[HlrEntry],
        llr: &[LlrEntry],
        tests: &[TestEntry],
    ) {
        std::fs::create_dir_all(dir).unwrap();
        let meta = || crate::trace::TraceMeta {
            document_id: "FIXTURE".into(),
            revision: "1".into(),
        };
        let schema = || crate::trace::Schema {
            version: SCHEMA_V.into(),
        };
        std::fs::write(
            dir.join("sys.toml"),
            toml::to_string_pretty(&crate::trace::HlrFile {
                schema: schema(),
                meta: meta(),
                requirements: sys.to_vec(),
            })
            .unwrap(),
        )
        .unwrap();
        std::fs::write(
            dir.join("hlr.toml"),
            toml::to_string_pretty(&crate::trace::HlrFile {
                schema: schema(),
                meta: meta(),
                requirements: hlr.to_vec(),
            })
            .unwrap(),
        )
        .unwrap();
        std::fs::write(
            dir.join("llr.toml"),
            toml::to_string_pretty(&crate::trace::LlrFile {
                schema: schema(),
                meta: meta(),
                requirements: llr.to_vec(),
            })
            .unwrap(),
        )
        .unwrap();
        std::fs::write(
            dir.join("tests.toml"),
            toml::to_string_pretty(&crate::trace::TestsFile {
                schema: schema(),
                meta: meta(),
                tests: tests.to_vec(),
            })
            .unwrap(),
        )
        .unwrap();
    }

    fn hlr_entry(uid: &str, id: &str, owner: &str, traces_to: Vec<String>) -> HlrEntry {
        HlrEntry {
            uid: Some(uid.into()),
            ns: None,
            id: id.into(),
            title: id.into(),
            owner: Some(owner.into()),
            scope: None,
            sort_key: None,
            category: None,
            source: None,
            description: None,
            rationale: None,
            verification_methods: vec![],
            traces_to,
            surfaces: vec![],
        }
    }

    fn llr_entry(uid: &str, id: &str, owner: &str, traces_to: Vec<String>) -> LlrEntry {
        LlrEntry {
            uid: Some(uid.into()),
            ns: None,
            id: id.into(),
            title: id.into(),
            owner: Some(owner.into()),
            sort_key: None,
            traces_to,
            source: None,
            modules: vec![],
            description: None,
            verification_methods: vec![],
            emits: vec![],
        }
    }

    fn test_entry(uid: &str, id: &str, owner: &str, traces_to: Vec<String>) -> TestEntry {
        TestEntry {
            uid: Some(uid.into()),
            ns: None,
            id: id.into(),
            title: id.into(),
            owner: Some(owner.into()),
            sort_key: None,
            traces_to,
            description: None,
            category: None,
            test_selector: None,
            test_selectors: vec![],
            source: None,
        }
    }

    /// One SYS + one HLR + one LLR + one TEST, fully linked
    /// upward, all owned by `tool` except the SYS (owner `soi`,
    /// which the HLR→SYS ownership rule admits).
    fn minimal_linked_entries() -> (HlrEntry, HlrEntry, LlrEntry, TestEntry) {
        let sys = hlr_entry(
            "aaaaaaaa-0000-4000-8000-000000000001",
            "SYS-1",
            "soi",
            vec![],
        );
        let hlr = hlr_entry(
            "aaaaaaaa-0000-4000-8000-000000000002",
            "HLR-1",
            "tool",
            vec![sys.uid.clone().unwrap()],
        );
        let llr = llr_entry(
            "aaaaaaaa-0000-4000-8000-000000000003",
            "LLR-1",
            "tool",
            vec![hlr.uid.clone().unwrap()],
        );
        let test = test_entry(
            "aaaaaaaa-0000-4000-8000-000000000004",
            "TEST-1",
            "tool",
            vec![llr.uid.clone().unwrap()],
        );
        (sys, hlr, llr, test)
    }

    /// No roots at all → NotConfigured. This is the state a
    /// requested validation over an unconfigured project must
    /// report instead of silently passing.
    #[test]
    fn no_roots_is_not_configured() {
        let eval = evaluate_trace_evidence(&[], &TracePolicy::default());
        assert_eq!(eval.state, TraceEvidenceState::NotConfigured);
        assert!(!eval.satisfies_claim());
        assert_eq!(eval.state.gap_code(), Some("TRACE_EVIDENCE_NOT_CONFIGURED"));
    }

    /// Every configured root missing on disk → NotAdopted, with
    /// the missing roots carried for the diagnostic message.
    #[test]
    fn all_roots_missing_is_not_adopted() {
        let tmp = tempfile::TempDir::new().unwrap();
        let missing = tmp.path().join("no/such/root").display().to_string();
        let eval = evaluate_trace_evidence(std::slice::from_ref(&missing), &TracePolicy::default());
        assert_eq!(
            eval.state,
            TraceEvidenceState::NotAdopted {
                missing_roots: vec![missing.clone()]
            }
        );
        assert_eq!(eval.missing_roots, vec![missing]);
        assert!(!eval.satisfies_claim());
        assert_eq!(eval.state.gap_code(), Some("TRACE_EVIDENCE_NOT_ADOPTED"));
    }

    /// Roots present but every layer empty → Empty. Pre-fix this
    /// validated vacuously and could terminate VERIFY_OK.
    #[test]
    fn zero_requirements_is_empty() {
        let tmp = tempfile::TempDir::new().unwrap();
        seed_root(tmp.path(), &[], &[], &[], &[]);
        let roots = vec![tmp.path().display().to_string()];
        let eval = evaluate_trace_evidence(&roots, &TracePolicy::default());
        assert_eq!(eval.state, TraceEvidenceState::Empty);
        assert_eq!(eval.requirement_count, 0);
        assert!(!eval.satisfies_claim());
        assert_eq!(eval.state.gap_code(), Some("TRACE_EVIDENCE_EMPTY"));
    }

    /// Non-empty evidence with a dangling link → Invalid, and the
    /// typed validation error is carried for per-variant emission.
    #[test]
    fn validation_failure_is_invalid() {
        let tmp = tempfile::TempDir::new().unwrap();
        let hlr = hlr_entry(
            "aaaaaaaa-0000-4000-8000-000000000002",
            "HLR-1",
            "tool",
            // Dangling: no SYS with this uid exists.
            vec!["bbbbbbbb-0000-4000-8000-000000000009".into()],
        );
        seed_root(tmp.path(), &[], std::slice::from_ref(&hlr), &[], &[]);
        let roots = vec![tmp.path().display().to_string()];
        let eval = evaluate_trace_evidence(&roots, &TracePolicy::default());
        assert_eq!(eval.state, TraceEvidenceState::Invalid);
        assert!(eval.validation.is_some());
        assert!(!eval.satisfies_claim());
        assert_eq!(eval.state.gap_code(), None);
    }

    /// The positive minimal graph: one SYS + one HLR + one LLR +
    /// one TEST fully linked → Valid. This is the boundary between
    /// "empty" and "valid" — the smallest evidence set that may
    /// satisfy a claim.
    #[test]
    fn minimal_linked_graph_is_valid() {
        let tmp = tempfile::TempDir::new().unwrap();
        let (sys, hlr, llr, test) = minimal_linked_entries();
        seed_root(
            tmp.path(),
            std::slice::from_ref(&sys),
            std::slice::from_ref(&hlr),
            std::slice::from_ref(&llr),
            std::slice::from_ref(&test),
        );
        let roots = vec![tmp.path().display().to_string()];
        let eval = evaluate_trace_evidence(&roots, &TracePolicy::default());
        assert_eq!(eval.state, TraceEvidenceState::Valid);
        assert_eq!(eval.requirement_count, 4);
        assert!(eval.satisfies_claim());
        assert!(eval.validation.is_none());
    }

    /// Root list order is non-semantic: two present roots (one
    /// empty, one holding the minimal graph) plus one missing root
    /// classify identically regardless of their order in the list.
    #[test]
    fn root_order_is_non_semantic() {
        let tmp = tempfile::TempDir::new().unwrap();
        let empty_root = tmp.path().join("empty");
        let graph_root = tmp.path().join("graph");
        seed_root(&empty_root, &[], &[], &[], &[]);
        let (sys, hlr, llr, test) = minimal_linked_entries();
        seed_root(
            &graph_root,
            &[sys],
            std::slice::from_ref(&hlr),
            std::slice::from_ref(&llr),
            std::slice::from_ref(&test),
        );
        let missing = tmp.path().join("missing").display().to_string();

        let forward = vec![
            empty_root.display().to_string(),
            graph_root.display().to_string(),
            missing.clone(),
        ];
        let mut reversed = forward.clone();
        reversed.reverse();

        let a = evaluate_trace_evidence(&forward, &TracePolicy::default());
        let b = evaluate_trace_evidence(&reversed, &TracePolicy::default());
        assert_eq!(a.state, b.state);
        assert_eq!(a.requirement_count, b.requirement_count);
        let mut a_missing = a.missing_roots.clone();
        let mut b_missing = b.missing_roots.clone();
        a_missing.sort();
        b_missing.sort();
        assert_eq!(a_missing, b_missing);
        assert_eq!(a.state, TraceEvidenceState::Valid);
        assert_eq!(a_missing, vec![missing]);
    }
}
