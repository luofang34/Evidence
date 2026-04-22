//! Subprocess capture helper backing
//! [`crate::bundle::EvidenceBuilder::run_capture`].
//!
//! Pure function: given a [`Command`] + output-filename hints +
//! the bundle directory, spawn the subprocess, LF-normalize
//! stdout/stderr, write both to bundle-relative paths, and
//! return a [`RunCaptureOutcome`]. The caller (the builder's
//! thin method wrapper) pushes the derived
//! [`crate::bundle::CommandRecord`] to `self.commands` and —
//! on non-zero exit — also records a
//! [`crate::bundle::ToolCommandFailure`] on
//! `self.tool_command_failures`.
//!
//! Split here to keep `bundle/builder.rs` under the 500-line
//! file-size limit; semantically the capture concern is a
//! clean carve-out from builder lifecycle (subprocess
//! mechanics, not state-assembly).

use std::fs;
use std::path::Path;
use std::process::Command;

use super::capture::normalize_captured_text;
use super::command::CommandRecord;
use super::command_failure::{ToolCommandFailure, tail_stderr};
use super::error::BuilderError;

/// Result of a captured subprocess run.
pub struct RunCaptureOutcome {
    /// CommandRecord suitable for `commands.json` — the caller
    /// pushes this onto `self.commands`.
    pub record: CommandRecord,
    /// Non-empty when the subprocess exited non-zero — caller
    /// pushes this onto `self.tool_command_failures`.
    pub failure: Option<ToolCommandFailure>,
    /// LF-normalized stdout, returned so the caller can parse.
    pub stdout_norm: Vec<u8>,
    /// LF-normalized stderr, returned so the caller can parse.
    pub stderr_norm: Vec<u8>,
}

/// Run `cmd`, write captured stdout/stderr to
/// `<bundle_dir>/<rel_dir>/<base>_stdout.txt` +
/// `<bundle_dir>/<rel_dir>/<base>_stderr.txt` (or base-relative
/// names when `rel_dir` is empty), and return the outcome.
/// Library layer — no presentation logging.
pub fn run_capture(
    mut cmd: Command,
    rel_dir: &str,
    output_name_base: &str,
    display_name: &str,
    bundle_dir: &Path,
) -> Result<RunCaptureOutcome, BuilderError> {
    let cwd = std::env::current_dir()
        .map_err(BuilderError::CurrentDir)?
        .display()
        .to_string();
    let argv = {
        let mut v = Vec::new();
        v.push(cmd.get_program().to_string_lossy().to_string());
        v.extend(cmd.get_args().map(|a| a.to_string_lossy().to_string()));
        v
    };

    tracing::info!("evidence: running {}...", display_name);
    let output = cmd.output().map_err(|source| BuilderError::RunCommand {
        display_name: display_name.to_string(),
        source,
    })?;
    let exit_code = output.status.code().unwrap_or(-1);
    let subprocess_failed = !output.status.success();

    let (stdout_path, stderr_path) = if rel_dir.is_empty() {
        (
            Some(format!("{}.json", output_name_base)),
            Some(format!("{}_stderr.txt", output_name_base)),
        )
    } else {
        (
            Some(format!("{}/{}_stdout.txt", rel_dir, output_name_base)),
            Some(format!("{}/{}_stderr.txt", rel_dir, output_name_base)),
        )
    };

    // Captured text is CRLF→LF normalized before being written
    // so the same logical run on Windows and Linux produces
    // byte-identical files (and therefore a stable content_hash).
    // See README "Captured Output Normalization".
    let stdout_norm = normalize_captured_text(&output.stdout);
    let stderr_norm = normalize_captured_text(&output.stderr);

    write_under_bundle(bundle_dir, stdout_path.as_deref(), &stdout_norm, "stdout")?;
    write_under_bundle(bundle_dir, stderr_path.as_deref(), &stderr_norm, "stderr")?;

    let record = CommandRecord {
        argv,
        cwd,
        exit_code,
        stdout_path,
        stderr_path,
    };

    let failure = if subprocess_failed {
        let stderr_text = String::from_utf8_lossy(&stderr_norm);
        Some(ToolCommandFailure {
            command_name: display_name.to_string(),
            exit_code,
            stderr_tail: tail_stderr(&stderr_text),
        })
    } else {
        None
    };

    Ok(RunCaptureOutcome {
        record,
        failure,
        stdout_norm,
        stderr_norm,
    })
}

fn write_under_bundle(
    bundle_dir: &Path,
    rel: Option<&str>,
    body: &[u8],
    kind: &'static str,
) -> Result<(), BuilderError> {
    let Some(rel) = rel else {
        return Ok(());
    };
    let abs = bundle_dir.join(rel);
    if let Some(parent) = abs.parent() {
        fs::create_dir_all(parent).map_err(|source| BuilderError::Io {
            op: "creating",
            path: parent.to_path_buf(),
            source,
        })?;
    }
    let op: &'static str = if kind == "stdout" {
        "writing stdout to"
    } else {
        "writing stderr to"
    };
    fs::write(&abs, body).map_err(|source| BuilderError::Io {
        op,
        path: abs.clone(),
        source,
    })?;
    Ok(())
}
