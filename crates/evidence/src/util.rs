//! Shared utility functions.

use anyhow::{bail, Result};
use std::process::Command;

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
