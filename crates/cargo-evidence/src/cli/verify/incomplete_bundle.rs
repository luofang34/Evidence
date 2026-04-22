//! Helper for `verify`'s dev-profile `bundle_complete=false`
//! Warning emission.
//!
//! Carved out of `cli/verify.rs` to keep that file under the
//! 500-line limit. The library side (`check_bundle_completeness`)
//! stays policy-free — it only pushes VerifyError variants for
//! cert/record-profile bundles — so the dev-profile Warning
//! handling lives here in the CLI.

use std::path::Path;

use anyhow::Result;
use evidence_core::{Diagnostic, Location, Severity};

use crate::cli::output::emit_jsonl;

/// Parse the bundle's `index.json` and, if `bundle_complete` is
/// false (dev-profile snapshot of a broken build), emit a
/// `VERIFY_BUNDLE_INCOMPLETE` Warning. Silent no-op when the
/// bundle is complete or the index can't be parsed — the
/// library already ran its own parse; if THAT failed verify
/// would have returned Err, not Pass.
pub fn maybe_emit_bundle_incomplete_warning(bundle_path: &Path) -> Result<()> {
    let idx_path = bundle_path.join("index.json");
    let Ok(body) = std::fs::read_to_string(&idx_path) else {
        return Ok(());
    };
    let Ok(idx) = serde_json::from_str::<evidence_core::EvidenceIndex>(&body) else {
        return Ok(());
    };
    if idx.bundle_complete {
        return Ok(());
    }
    let commands: Vec<String> = idx
        .tool_command_failures
        .iter()
        .map(|f| f.command_name.clone())
        .collect();
    emit_jsonl(&Diagnostic {
        code: "VERIFY_BUNDLE_INCOMPLETE".to_string(),
        severity: Severity::Warning,
        message: format!(
            "bundle_complete=false; {} captured command(s) exited non-zero: {}",
            commands.len(),
            commands.join(", ")
        ),
        location: Some(Location {
            file: Some(idx_path),
            ..Location::default()
        }),
        fix_hint: None,
        subcommand: None,
        root_cause_uid: None,
    })?;
    Ok(())
}
