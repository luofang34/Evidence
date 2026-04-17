//! Shared utility functions.

use anyhow::{Result, bail};
use std::path::Path;
use std::process::Command;

/// Normalize a bundle-relative path for on-disk serialization.
///
/// Every relative path that ends up in SHA256SUMS, `index.json`
/// (`trace_outputs`, `*_file`), `inputs_hashes.json`, or
/// `outputs_hashes.json` **must** flow through this helper before it is
/// written. Bundles travel across operating systems; a Windows producer
/// that leaves backslashes in the JSON makes the bundle unverifiable on
/// Linux (and vice-versa), because `is_safe_bundle_path` on the verify
/// side treats `\` as unsafe.
///
/// Rule: strip base → call this → then serialize. Never serialize a
/// `PathBuf` or `to_string_lossy()` result directly.
pub fn normalize_bundle_path(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

/// Run a command and capture stdout as a string.
///
/// Returns an error if the command fails (non-zero exit code) or
/// cannot be executed.
pub fn cmd_stdout(prog: &str, args: &[&str]) -> Result<String> {
    let out = Command::new(prog).args(args).output()?;
    if !out.status.success() {
        bail!("{} {:?} failed", prog, args);
    }
    String::from_utf8(out.stdout)
        .map_err(|_| anyhow::anyhow!("{} {:?} produced non-UTF-8 output", prog, args))
}
