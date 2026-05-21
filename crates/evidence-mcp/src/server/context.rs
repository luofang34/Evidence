//! Helpers backing the [`crate::Server::evidence_context`] tool
//! method. Lives next to `responses.rs` so the parent module
//! stays under the workspace 500-line limit while the handler-
//! specific argument-mapping logic gets its own home.
//!
//! The two exposed items both serve the handler exclusively:
//! [`SelectorArg`] mirrors the three equivalent CLI entry points
//! (`<positional>` / `--crate` / `--module`); [`pick_selector_arg`]
//! validates the [`crate::schema::ContextRequest`] selector trio
//! and lifts the chosen field into the enum.

use crate::schema::ContextRequest;

/// Disambiguated selector argument to pass to `cargo evidence
/// context`. The three variants correspond 1:1 with the three
/// equivalent entry points exposed by the CLI (`<positional>`,
/// `--crate`, `--module`).
#[derive(Debug)]
pub(super) enum SelectorArg {
    /// Positional argument — free-form selector resolved by
    /// priority (file > crate > module).
    Positional(String),
    /// `--crate <name>` disambiguator.
    Crate(String),
    /// `--module <path>` disambiguator.
    Module(String),
}

impl SelectorArg {
    /// Append the CLI arguments this selector represents onto an
    /// existing arg vector. Keeps the handler-side dispatch
    /// shape-agnostic (the handler just calls `extend_args`
    /// regardless of the variant).
    pub(super) fn extend_args(self, args: &mut Vec<String>) {
        match self {
            Self::Crate(c) => {
                args.push("--crate".into());
                args.push(c);
            }
            Self::Module(m) => {
                args.push("--module".into());
                args.push(m);
            }
            Self::Positional(p) => {
                args.push(p);
            }
        }
    }
}

/// Validate the [`ContextRequest`] selector trio and pick the
/// single non-`None` field. Returns `Ok(None)` when all three are
/// absent (workspace overview) and `Err(String)` when more than
/// one is set — a host-contract error reported the same way as
/// `evidence_check`'s invalid `mode` value.
pub(super) fn pick_selector_arg(req: &ContextRequest) -> Result<Option<SelectorArg>, String> {
    let set: Vec<&str> = [
        req.selector.as_deref().map(|_| "selector"),
        req.crate_name.as_deref().map(|_| "crate_name"),
        req.module.as_deref().map(|_| "module"),
    ]
    .into_iter()
    .flatten()
    .collect();
    if set.len() > 1 {
        return Err(format!(
            "invalid request: at most one of selector / crate_name / module may be set; \
             got {set:?}"
        ));
    }
    if let Some(s) = req.selector.as_deref().filter(|s| !s.is_empty()) {
        return Ok(Some(SelectorArg::Positional(s.to_string())));
    }
    if let Some(c) = req.crate_name.as_deref().filter(|c| !c.is_empty()) {
        return Ok(Some(SelectorArg::Crate(c.to_string())));
    }
    if let Some(m) = req.module.as_deref().filter(|m| !m.is_empty()) {
        return Ok(Some(SelectorArg::Module(m.to_string())));
    }
    Ok(None)
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

    #[test]
    fn pick_selector_arg_none_when_all_unset() {
        let req = ContextRequest::default();
        let picked = pick_selector_arg(&req).expect("ok");
        assert!(
            picked.is_none(),
            "all-unset request should be workspace overview"
        );
    }

    #[test]
    fn pick_selector_arg_positional_wins_when_selector_set() {
        let req = ContextRequest {
            selector: Some("crates/x/src/y.rs".to_string()),
            ..Default::default()
        };
        let picked = pick_selector_arg(&req).expect("ok").expect("some");
        assert!(matches!(picked, SelectorArg::Positional(_)));
        let mut args = vec!["context".to_string()];
        picked.extend_args(&mut args);
        assert_eq!(args, vec!["context", "crates/x/src/y.rs"]);
    }

    #[test]
    fn pick_selector_arg_crate_flag_emits_crate_arg_pair() {
        let req = ContextRequest {
            crate_name: Some("evidence-mcp".to_string()),
            ..Default::default()
        };
        let picked = pick_selector_arg(&req).expect("ok").expect("some");
        let mut args = vec!["context".to_string()];
        picked.extend_args(&mut args);
        assert_eq!(args, vec!["context", "--crate", "evidence-mcp"]);
    }

    #[test]
    fn pick_selector_arg_module_flag_emits_module_arg_pair() {
        let req = ContextRequest {
            module: Some("evidence_core::trace".to_string()),
            ..Default::default()
        };
        let picked = pick_selector_arg(&req).expect("ok").expect("some");
        let mut args = vec!["context".to_string()];
        picked.extend_args(&mut args);
        assert_eq!(args, vec!["context", "--module", "evidence_core::trace"]);
    }

    #[test]
    fn pick_selector_arg_rejects_multiple_fields_set() {
        let req = ContextRequest {
            selector: Some("foo".to_string()),
            crate_name: Some("bar".to_string()),
            ..Default::default()
        };
        let err = pick_selector_arg(&req).expect_err("multiple set must fail");
        assert!(err.contains("at most one"));
        assert!(err.contains("selector"));
        assert!(err.contains("crate_name"));
    }
}
