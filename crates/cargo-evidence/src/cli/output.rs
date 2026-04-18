//! Output rendering primitives shared by every subcommand.
//!
//! Commands produce three kinds of output:
//!
//! - human-readable text (default);
//! - a single pretty-printed JSON document (`--format=json` / legacy
//!   `--json`), emitted by [`emit_json`];
//! - a stream of JSON-Lines diagnostics (`--format=jsonl`), emitted
//!   one record at a time by [`emit_jsonl`].
//!
//! Convention: machine-readable output goes to **stdout** (so callers
//! can pipe it through `jq`), text progress and error messages go to
//! **stderr**. The two streams never share content — a `--json`
//! invocation produces exactly one JSON document on stdout and nothing
//! else; a `--format=jsonl` invocation produces one JSON object per
//! line, one object per terminated line.

use std::io::{self, Write};

use anyhow::Result;
use serde::Serialize;

use evidence::Diagnostic;

/// Serialize `value` as pretty JSON and write it to stdout followed
/// by a single newline.
///
/// Used by every `--json` output path so the on-the-wire shape stays
/// identical across commands: one `serde_json::to_string_pretty` call,
/// one trailing LF. Callers that want stderr instead (e.g. an error
/// envelope on a non-zero exit) should print there directly — JSON on
/// stdout is reserved for the primary result.
pub fn emit_json<T: Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

/// Emit one [`Diagnostic`] as a single JSON-Lines record on stdout and
/// flush immediately.
///
/// Wire contract lives in `schemas/diagnostic.schema.json`:
///
/// - **stdout strict** (Schema Rule 2): one compact JSON object, no
///   pretty-printing, followed by exactly one `\n`. Block-buffered
///   stdout on a pipe would defeat agent streaming.
/// - **flush per event** (Schema Rule 4): the explicit `flush()` call
///   is load-bearing — without it a downstream reader blocked on
///   `readline()` can stall for minutes waiting for the OS to drain
///   the pipe buffer.
///
/// The caller is responsible for sequencing: the *last* line emitted
/// must be a terminal event whose code ends in `_OK` or `_FAIL`, per
/// Schema Rule 1.
pub fn emit_jsonl(diag: &Diagnostic) -> Result<()> {
    let line = serde_json::to_string(diag)?;
    let stdout = io::stdout();
    let mut handle = stdout.lock();
    writeln!(handle, "{}", line)?;
    handle.flush()?;
    Ok(())
}
