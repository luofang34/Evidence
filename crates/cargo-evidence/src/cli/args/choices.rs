//! Value-enum CLI argument types (`--coverage`, `--format`,
//! `--mode`). Split out of the parent `args.rs` so that file stays
//! under the workspace 500-line limit; re-exported from there so
//! consumers keep their `crate::cli::args::CoverageChoice` paths.

use clap::ValueEnum;

/// Selects how `check` interprets its path argument.
///
/// - `Auto` (default): inspect the path. Containing `SHA256SUMS`
///   wins (bundle mode); else containing `Cargo.toml` (source
///   mode); else `CLI_INVALID_ARGUMENT`.
/// - `Source`: force source mode; reject a bundle dir.
/// - `Bundle`: force bundle mode; reject a source tree.
#[derive(Clone, Copy, Default, PartialEq, Eq, ValueEnum, Debug)]
pub enum CheckMode {
    /// Pick mode from the path shape (default).
    #[default]
    Auto,
    /// Force source-tree mode (trace validation + test run).
    Source,
    /// Force bundle mode (delegate to `verify`).
    Bundle,
}

/// `--coverage` level for `cargo evidence generate`.
///
/// Controls the structural-coverage phase that runs between
/// `cargo test` and `finalize` when enabled. See HLR-053.
/// Captured percentages are engineering metrics only — no
/// percentage alone closes a DO-178C A-7 objective (LLR-108).
///
/// Default is resolved at runtime by profile: `Dev` → `None`
/// (fast iteration), `Cert`/`Record` → `Branch` (the broadest
/// approximation the tool captures for named-claim profiles).
/// Passing `--coverage` explicitly always wins.
#[derive(Clone, Copy, Default, PartialEq, Eq, ValueEnum, Debug)]
pub enum CoverageChoice {
    /// Do not run `cargo llvm-cov`; bundle carries no coverage
    /// artifact. Dev-profile default.
    #[default]
    None,
    /// Line / statement coverage only (A-7 Obj-5 dimension,
    /// engineering metric).
    Line,
    /// LLVM branch coverage — an approximation of decision
    /// coverage (A-7 Obj-6 dimension). Cert/record default.
    Branch,
    /// Emit both `line` and `branch` measurements from the
    /// same instrumented test pass.
    All,
}

/// Global `--format` choice. See [`super::EvidenceArgs::format`].
#[derive(Clone, Copy, Default, PartialEq, Eq, ValueEnum, Debug)]
pub enum OutputFormat {
    /// Human-readable text (default).
    #[default]
    Human,
    /// Single pretty-printed JSON document on stdout.
    Json,
    /// Streaming JSON-Lines on stdout, flushed per event.
    Jsonl,
}

impl OutputFormat {
    /// Resolve the effective output format from the possibly-multiple
    /// knobs the user may have set.
    ///
    /// Precedence:
    /// 1. If `--format` is anything other than its default (`Human`),
    ///    honor it (the user was explicit).
    /// 2. Otherwise, if the legacy `--json` boolean is set, treat as
    ///    `Json` (Schema Rule 5: permanent alias).
    /// 3. Otherwise `Human`.
    pub fn resolve(format_flag: OutputFormat, json_flag: bool) -> OutputFormat {
        if format_flag != OutputFormat::Human {
            return format_flag;
        }
        if json_flag {
            return OutputFormat::Json;
        }
        OutputFormat::Human
    }
}
