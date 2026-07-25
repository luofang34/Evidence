//! The bounded, offline, fail-closed PDF extractor runner
//! (LLR-180).
//!
//! [`run_pdftotext_blocking`] receives an explicit executable path
//! and an explicit verified PDF path — it never searches PATH (a
//! bare executable name is rejected before spawn), never passes a
//! shell, never passes `-nodrm`, passwords, or any
//! network-capable helper, and never mutates the workspace. The
//! child runs with its current directory set to a freshly created
//! isolated temporary directory and the pinned argv
//! ([`PINNED_ARGV`]) plus the explicit input and output paths.
//!
//! Execution is bounded: wall time (a polling wait loop that
//! kills the child on expiry), captured stdout and stderr bytes
//! (drained on threads so a chatty child cannot deadlock the
//! wait), output-file bytes, and produced file count (exactly one
//! output file). Every failure is a typed [`PdfRunError`] —
//! spawn, timeout, signal, exit code, missing output, oversized
//! output, and cleanup — and the function returns `Result`; it
//! never terminates the process.
//!
//! A nonzero exit is respected, including Poppler's documented
//! permission-related exits: the runner never retries with DRM
//! bypass flags.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use thiserror::Error;

use super::super::super::digest::StructuralContentDigest;
use super::lock::PINNED_ARGV;

/// The output file name inside the isolated temporary directory.
const OUTPUT_FILE_NAME: &str = "pdftotext-bbox-output.xhtml";

/// The poll interval of the wait-with-timeout loop.
const POLL_INTERVAL: Duration = Duration::from_millis(5);

/// The timeout of the `-v` version probe.
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);

/// Counter making the isolated temporary directory name unique
/// within one process.
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// The explicit bounds of one extractor run (LLR-180).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PdfRunBounds {
    /// Wall-time bound; expiry kills the child and reports
    /// [`PdfRunError::Timeout`].
    pub timeout: Duration,
    /// Captured stdout byte bound.
    pub max_stdout_bytes: usize,
    /// Captured stderr byte bound.
    pub max_stderr_bytes: usize,
    /// Extractor output-file byte bound.
    pub max_output_bytes: usize,
}

impl Default for PdfRunBounds {
    fn default() -> Self {
        PdfRunBounds {
            timeout: Duration::from_secs(60),
            max_stdout_bytes: 1024 * 1024,
            max_stderr_bytes: 1024 * 1024,
            max_output_bytes: 64 * 1024 * 1024,
        }
    }
}

/// The outcome of one bounded extractor run (LLR-180).
#[derive(Debug, Clone)]
pub struct PdfExtraction {
    /// The raw extractor output bytes (the bbox-layout XHTML).
    pub output_bytes: Vec<u8>,
    /// The raw extractor-output digest: SHA-256 over
    /// `output_bytes`; an output-identity component.
    pub output_digest: StructuralContentDigest,
    /// The bounded captured stdout.
    pub stdout: Vec<u8>,
    /// The bounded captured stderr.
    pub stderr: Vec<u8>,
}

/// Every fail-closed runner violation (LLR-180).
#[derive(Debug, Error)]
pub enum PdfRunError {
    /// The executable path is a bare name that would be resolved
    /// through PATH.
    #[error("executable path {path:?} is a bare name; an explicit path is required")]
    PathLookupForbidden {
        /// The offending path.
        path: String,
    },
    /// The isolated temporary directory could not be created.
    #[error("isolated temporary directory {path} could not be created: {source}")]
    TempDir {
        /// The directory path.
        path: String,
        /// The I/O failure.
        source: std::io::Error,
    },
    /// The child process could not be spawned.
    #[error("extractor {path} could not be spawned: {source}")]
    Spawn {
        /// The executable path.
        path: String,
        /// The I/O failure.
        source: std::io::Error,
    },
    /// The child exceeded the wall-time bound and was killed.
    #[error("extractor exceeded the {} ms wall-time bound and was killed", .timeout.as_millis())]
    Timeout {
        /// The configured bound.
        timeout: Duration,
    },
    /// The child exited nonzero (including Poppler's documented
    /// permission-related exits, which are respected, never
    /// bypassed).
    #[error("extractor exited with code {code:?}: {}", String::from_utf8_lossy(.stderr))]
    ExitCode {
        /// The exit code, when the platform reports one.
        code: Option<i32>,
        /// The bounded captured stderr.
        stderr: Vec<u8>,
    },
    /// The child was terminated by a signal.
    #[error("extractor was terminated by signal {signal}")]
    Signal {
        /// The terminating signal number.
        signal: i32,
    },
    /// The child exited zero but produced no output file.
    #[error("extractor exited zero but produced no output file in {dir}")]
    MissingOutput {
        /// The isolated directory that was searched.
        dir: String,
    },
    /// A bounded quantity exceeded its limit.
    #[error("extractor {what} exceeded its bound: {actual} > {limit}")]
    OversizedOutput {
        /// The bounded quantity: `stdout-bytes`, `stderr-bytes`,
        /// `output-bytes`, or `file-count`.
        what: &'static str,
        /// The observed value.
        actual: usize,
        /// The configured bound.
        limit: usize,
    },
    /// The isolated temporary directory could not be removed after
    /// an otherwise successful run.
    #[error("isolated temporary directory {path} could not be cleaned up: {source}")]
    Cleanup {
        /// The directory path.
        path: String,
        /// The I/O failure.
        source: std::io::Error,
    },
}

/// Run the locked extractor over `pdf_path` under `bounds`
/// (LLR-180). See the module docs for the contract.
///
/// # Errors
///
/// Fails closed with typed [`PdfRunError`] context on any
/// contract or bound violation; the child is always reaped.
pub fn run_pdftotext_blocking(
    executable: &Path,
    pdf_path: &Path,
    bounds: &PdfRunBounds,
) -> Result<PdfExtraction, PdfRunError> {
    reject_bare_name(executable)?;
    let dir = create_isolated_dir()?;
    let result = run_in_dir(executable, pdf_path, bounds, &dir);
    match result {
        Ok(extraction) => {
            std::fs::remove_dir_all(&dir).map_err(|source| PdfRunError::Cleanup {
                path: dir.display().to_string(),
                source,
            })?;
            Ok(extraction)
        }
        Err(error) => {
            // Best-effort cleanup on the failure path: the typed
            // error already carries the primary context and must
            // not be masked.
            drop(std::fs::remove_dir_all(&dir));
            Err(error)
        }
    }
}

/// Probe the explicit executable's `-v` output, returning its
/// first line (LLR-179). Used by the tool lock's executable
/// verification; the probe is bounded by [`PROBE_TIMEOUT`] and
/// never searches PATH.
///
/// # Errors
///
/// Returns the spawn, wait, or non-success failure as plain I/O
/// context; the caller wraps it in its own typed error.
pub(crate) fn probe_version_blocking(executable: &Path) -> std::io::Result<String> {
    reject_bare_name_io(executable)?;
    let mut child = Command::new(executable)
        .arg("-v")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let stdout = drain_bounded(child.stdout.take(), 64 * 1024);
    let stderr = drain_bounded(child.stderr.take(), 64 * 1024);
    let status = wait_with_timeout(&mut child, PROBE_TIMEOUT)?;
    let captured_stdout = stdout.join().unwrap_or_default();
    let captured_stderr = stderr.join().unwrap_or_default();
    if !status.success() {
        return Err(std::io::Error::other(format!(
            "-v probe exited {status}: {}",
            String::from_utf8_lossy(&captured_stderr)
        )));
    }
    // Poppler prints its version banner to stderr.
    let combined = if captured_stderr.is_empty() {
        captured_stdout
    } else {
        captured_stderr
    };
    let text = String::from_utf8_lossy(&combined);
    Ok(text.lines().next().unwrap_or("").trim_end().to_string())
}

/// A bare executable name (no path separator) is a PATH lookup;
/// the runner contract forbids it.
fn reject_bare_name(executable: &Path) -> Result<(), PdfRunError> {
    if executable.components().count() < 2 {
        return Err(PdfRunError::PathLookupForbidden {
            path: executable.display().to_string(),
        });
    }
    Ok(())
}

fn reject_bare_name_io(executable: &Path) -> std::io::Result<()> {
    reject_bare_name(executable).map_err(|error| std::io::Error::other(error.to_string()))
}

/// Create a fresh isolated temporary directory for one run.
fn create_isolated_dir() -> Result<PathBuf, PdfRunError> {
    let sequence = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.subsec_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!(
        "evidence-pdf-extract-{}-{sequence}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir(&dir).map_err(|source| PdfRunError::TempDir {
        path: dir.display().to_string(),
        source,
    })?;
    Ok(dir)
}

/// Run the child inside the isolated directory and collect the
/// bounded outcome.
fn run_in_dir(
    executable: &Path,
    pdf_path: &Path,
    bounds: &PdfRunBounds,
    dir: &Path,
) -> Result<PdfExtraction, PdfRunError> {
    let output_path = dir.join(OUTPUT_FILE_NAME);
    let mut command = Command::new(executable);
    command
        .args(PINNED_ARGV)
        .arg(pdf_path)
        .arg(&output_path)
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|source| PdfRunError::Spawn {
        path: executable.display().to_string(),
        source,
    })?;
    let stdout = drain_bounded(child.stdout.take(), bounds.max_stdout_bytes);
    let stderr = drain_bounded(child.stderr.take(), bounds.max_stderr_bytes);
    let wait = wait_with_timeout_io(&mut child, bounds.timeout);
    let captured_stdout = stdout.join().unwrap_or_default();
    let captured_stderr = stderr.join().unwrap_or_default();
    let status = match wait {
        Ok(status) => status,
        Err(PdfRunError::Timeout { timeout }) => return Err(PdfRunError::Timeout { timeout }),
        Err(PdfRunError::Spawn { source, .. }) => {
            // A plain wait failure is spawn-adjacent.
            drop(child.kill());
            drop(child.wait());
            return Err(PdfRunError::Spawn {
                path: executable.display().to_string(),
                source,
            });
        }
        Err(other) => return Err(other),
    };
    if captured_stdout.len() > bounds.max_stdout_bytes {
        return Err(PdfRunError::OversizedOutput {
            what: "stdout-bytes",
            actual: captured_stdout.len(),
            limit: bounds.max_stdout_bytes,
        });
    }
    if captured_stderr.len() > bounds.max_stderr_bytes {
        return Err(PdfRunError::OversizedOutput {
            what: "stderr-bytes",
            actual: captured_stderr.len(),
            limit: bounds.max_stderr_bytes,
        });
    }
    if !status.success() {
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            if let Some(signal) = status.signal() {
                return Err(PdfRunError::Signal { signal });
            }
        }
        return Err(PdfRunError::ExitCode {
            code: status.code(),
            stderr: captured_stderr,
        });
    }
    let produced: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|source| PdfRunError::TempDir {
            path: dir.display().to_string(),
            source,
        })?
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.path())
        .collect();
    if produced.len() > 1 {
        return Err(PdfRunError::OversizedOutput {
            what: "file-count",
            actual: produced.len(),
            limit: 1,
        });
    }
    if !output_path.is_file() {
        return Err(PdfRunError::MissingOutput {
            dir: dir.display().to_string(),
        });
    }
    let output_bytes = std::fs::read(&output_path).map_err(|source| PdfRunError::Spawn {
        path: output_path.display().to_string(),
        source,
    })?;
    if output_bytes.len() > bounds.max_output_bytes {
        return Err(PdfRunError::OversizedOutput {
            what: "output-bytes",
            actual: output_bytes.len(),
            limit: bounds.max_output_bytes,
        });
    }
    let output_digest =
        StructuralContentDigest::from_hasher_output(crate::hash::sha256(&output_bytes));
    Ok(PdfExtraction {
        output_bytes,
        output_digest,
        stdout: captured_stdout,
        stderr: captured_stderr,
    })
}

/// Wait for `child`, polling every [`POLL_INTERVAL`]; on timeout
/// kill and reap the child and report [`PdfRunError::Timeout`].
fn wait_with_timeout_io(
    child: &mut Child,
    timeout: Duration,
) -> Result<std::process::ExitStatus, PdfRunError> {
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => {
                if start.elapsed() >= timeout {
                    drop(child.kill());
                    drop(child.wait());
                    return Err(PdfRunError::Timeout { timeout });
                }
                std::thread::sleep(POLL_INTERVAL);
            }
            Err(source) => {
                return Err(PdfRunError::Spawn {
                    path: "child".to_string(),
                    source,
                });
            }
        }
    }
}

/// `wait_with_timeout` as a plain I/O result for the version
/// probe: timeout kills the child and surfaces as an error.
fn wait_with_timeout(
    child: &mut Child,
    timeout: Duration,
) -> std::io::Result<std::process::ExitStatus> {
    wait_with_timeout_io(child, timeout).map_err(|error| match error {
        PdfRunError::Spawn { source, .. } => source,
        other => std::io::Error::other(other.to_string()),
    })
}

/// Drain a child pipe on a thread so a chatty child cannot block
/// on a full pipe; the first `cap + 1` bytes are kept so the
/// caller can detect the bound violation.
fn drain_bounded<R: Read + Send + 'static>(
    reader: Option<R>,
    cap: usize,
) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut kept = Vec::new();
        let Some(mut reader) = reader else {
            return kept;
        };
        let mut chunk = [0_u8; 8192];
        loop {
            match reader.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    let remaining = (cap + 1).saturating_sub(kept.len());
                    let take = remaining.min(read);
                    kept.extend_from_slice(&chunk[..take]);
                }
            }
        }
        kept
    })
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
#[path = "runner_tests.rs"]
mod tests;
