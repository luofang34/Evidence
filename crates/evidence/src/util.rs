//! Shared utility functions.

use std::path::Path;
use std::process::Command;

use thiserror::Error;

/// Errors returned by [`cmd_stdout`].
#[derive(Debug, Error)]
pub enum CmdError {
    /// The child process failed to launch (e.g. binary not found).
    #[error("failed to launch {prog} {args:?}")]
    Launch {
        /// Program name passed to `Command::new`.
        prog: String,
        /// Arguments passed to the program.
        args: Vec<String>,
        /// Underlying spawn error.
        #[source]
        source: std::io::Error,
    },
    /// The child process ran but exited with a non-zero status.
    #[error("{prog} {args:?} failed with {status}")]
    NonZeroExit {
        /// Program name passed to `Command::new`.
        prog: String,
        /// Arguments passed to the program.
        args: Vec<String>,
        /// Non-zero exit status observed.
        status: std::process::ExitStatus,
    },
    /// Stdout contained bytes that are not valid UTF-8.
    #[error("{prog} {args:?} produced non-UTF-8 output")]
    NonUtf8Output {
        /// Program name passed to `Command::new`.
        prog: String,
        /// Arguments passed to the program.
        args: Vec<String>,
        /// Underlying UTF-8 decode error.
        #[source]
        source: std::string::FromUtf8Error,
    },
}

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
/// Returns an error if the command fails to launch, exits with non-zero
/// status, or emits non-UTF-8 stdout.
pub fn cmd_stdout(prog: &str, args: &[&str]) -> Result<String, CmdError> {
    let owned_args: Vec<String> = args.iter().map(|s| (*s).to_string()).collect();
    let out = Command::new(prog)
        .args(args)
        .output()
        .map_err(|source| CmdError::Launch {
            prog: prog.to_string(),
            args: owned_args.clone(),
            source,
        })?;
    if !out.status.success() {
        return Err(CmdError::NonZeroExit {
            prog: prog.to_string(),
            args: owned_args,
            status: out.status,
        });
    }
    String::from_utf8(out.stdout).map_err(|source| CmdError::NonUtf8Output {
        prog: prog.to_string(),
        args: owned_args,
        source,
    })
}
