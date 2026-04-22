//! `ToolCommandFailure` — structured record of a captured
//! subprocess that exited non-zero (cargo test failing, cargo
//! check failing, cargo build failing).
//!
//! Previously a non-zero exit from `run_capture` was surfaced as
//! a library-layer `tracing::error!` log and otherwise swallowed;
//! a downstream `verify` call on the resulting bundle could not
//! tell the bundle represented a broken build. Recording the
//! failure structurally on [`crate::bundle::EvidenceBuilder`] +
//! mirroring to [`crate::bundle::EvidenceIndex.tool_command_failures`]
//! is what lets `verify` cross-check "the bundle claims
//! complete but a captured command failed" (VERIFY_BUNDLE_INCOMPLETELY_CLAIMED)
//! and refuse cert/record bundles with any recorded failure
//! (VERIFY_TOOL_COMMANDS_FAILED_SILENTLY).
//!
//! **Wire contract — `stderr_tail` truncation.** The recorded
//! stderr is the last [`STDERR_TAIL_LINES`] lines of the
//! subprocess's stderr, not the full body. The cap keeps the
//! bundle size bounded (a 10k-line cargo-test build failure
//! would otherwise inflate `index.json` past useful-review
//! size) while preserving the panic / error block that
//! typically appears near the end. Pinned by unit test; bumping
//! the cap is a wire-contract change requiring a deliberate
//! edit here + baseline regen.
//!
//! If the full stderr is needed, the bundle also contains it
//! verbatim at `commands.json[].stderr_path` → that artifact is
//! integrity-hashed into SHA256SUMS as usual.

use serde::{Deserialize, Serialize};

/// Number of trailing stderr lines captured into
/// [`ToolCommandFailure::stderr_tail`]. The full stderr remains
/// available on disk at the corresponding
/// `commands.json[].stderr_path` artifact.
pub const STDERR_TAIL_LINES: usize = 20;

/// One captured-subprocess failure. Appears as an entry in
/// [`crate::bundle::EvidenceIndex::tool_command_failures`] and
/// drives the verify-time cross-check.
///
/// Field order here matches the JSON key order on the wire
/// (serde_json preserves struct field order on serialize).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCommandFailure {
    /// Display name of the failing command, as passed to
    /// `run_capture`'s `display_name` argument. Example:
    /// `"cargo test --workspace"`.
    pub command_name: String,

    /// Process exit code. `-1` when the subprocess didn't
    /// expose one (killed by signal on Unix, or spawn failure
    /// where no process ever ran).
    pub exit_code: i32,

    /// Last [`STDERR_TAIL_LINES`] lines of the subprocess's
    /// stderr, joined with `\n`. CRLF-normalized to LF by the
    /// caller so cross-host bundles produce byte-identical
    /// tails. Empty string when stderr was empty.
    pub stderr_tail: String,
}

/// Select the last [`STDERR_TAIL_LINES`] lines of `stderr_text`
/// into the wire-contract-pinned `stderr_tail` form.
pub fn tail_stderr(stderr_text: &str) -> String {
    let lines: Vec<&str> = stderr_text.lines().collect();
    let start = lines.len().saturating_sub(STDERR_TAIL_LINES);
    lines[start..].join("\n")
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

    /// `tail_stderr` returns the last N lines where N =
    /// [`STDERR_TAIL_LINES`]. Longer input is truncated from
    /// the FRONT (preserves the final panic / error block).
    #[test]
    fn tail_stderr_keeps_last_twenty_lines() {
        let body: String = (1..=50).map(|i| format!("line {i}\n")).collect();
        let tail = tail_stderr(&body);
        let lines: Vec<&str> = tail.lines().collect();
        assert_eq!(lines.len(), STDERR_TAIL_LINES, "cap at {STDERR_TAIL_LINES}");
        assert_eq!(
            lines.first().copied(),
            Some("line 31"),
            "kept lines 31..=50, dropped 1..=30",
        );
        assert_eq!(lines.last().copied(), Some("line 50"));
    }

    /// Short input is returned unchanged.
    #[test]
    fn tail_stderr_shorter_than_cap_is_unchanged() {
        let body = "a\nb\nc\n";
        let tail = tail_stderr(body);
        assert_eq!(tail.lines().count(), 3);
        assert_eq!(tail, "a\nb\nc");
    }

    /// Empty input produces empty tail (not a panic from
    /// `saturating_sub` or slicing).
    #[test]
    fn tail_stderr_empty_input_empty_tail() {
        assert_eq!(tail_stderr(""), "");
    }

    /// Roundtrip through serde — wire contract is stable.
    #[test]
    fn roundtrip_json() {
        let rec = ToolCommandFailure {
            command_name: "cargo test --workspace".to_string(),
            exit_code: 101,
            stderr_tail: "error[E0432]: unresolved import\n".to_string(),
        };
        let wire = serde_json::to_string(&rec).expect("serialize");
        let back: ToolCommandFailure = serde_json::from_str(&wire).expect("deserialize");
        assert_eq!(back, rec);
        // Wire field order — `command_name` first, `exit_code`,
        // `stderr_tail` last.
        let kn = wire.find("\"command_name\"").expect("command_name present");
        let ke = wire.find("\"exit_code\"").expect("exit_code present");
        let ks = wire.find("\"stderr_tail\"").expect("stderr_tail present");
        assert!(kn < ke && ke < ks, "wire field order: {wire}");
    }

    /// `stderr_tail` cap pin. A future PR that bumps the cap
    /// fires this test as its first warning — bumping requires
    /// a deliberate edit here and a baseline regen.
    #[test]
    fn stderr_tail_lines_cap_pinned_at_twenty() {
        assert_eq!(
            STDERR_TAIL_LINES, 20,
            "STDERR_TAIL_LINES is a wire contract — changing it rotates \
             existing bundles' tool_command_failures payloads. See the module \
             docstring for the protocol."
        );
    }
}
