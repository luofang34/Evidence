//! Output rendering primitives shared by every subcommand.
//!
//! Commands produce two kinds of output: machine-readable JSON (when
//! the user passes `--json`) and human-readable text otherwise. The
//! dispatch point lives here so every command routes through the same
//! writer and the same serde configuration; individual commands keep
//! only their own typed result types and their own text rendering.
//!
//! Convention: JSON goes to **stdout** (so callers can pipe it
//! through `jq`), text progress and error messages go to **stderr**.
//! The two streams never share content — a `--json` invocation
//! produces exactly one JSON document on stdout and nothing else.

use anyhow::Result;
use serde::Serialize;

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
