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
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}
