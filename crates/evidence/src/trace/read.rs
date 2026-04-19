//! TOML reading helpers for trace files.
//!
//! Missing files are tolerated with empty defaults + a warning, so a
//! boundary config that points at an empty trace root still produces
//! a readable `TraceFiles` rather than a bail. The `backfill_uuids`
//! and validation passes handle the empty case gracefully.

use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;

use super::entries::{DerivedFile, HlrFile, LlrFile, Schema, TestsFile, TraceMeta};
use crate::diagnostic::{DiagnosticCode, Location, Severity};

/// Errors returned by [`read_toml`] / [`read_all_trace_files`].
#[derive(Debug, Error)]
pub enum TraceReadError {
    /// Failed to read the TOML file from disk.
    #[error("reading {path}")]
    Read {
        /// Path whose read failed.
        path: PathBuf,
        /// Underlying OS error.
        #[source]
        source: std::io::Error,
    },
    /// The file read but its contents didn't parse as TOML for the
    /// requested target type.
    ///
    /// `toml::de::Error` is >128 bytes on Windows, which would push
    /// the whole enum past clippy's `result_large_err` threshold and
    /// force every `Result<_, TraceReadError>` to be heavier than
    /// necessary. Box it so the error variant stays cheap to return.
    #[error("parsing {path}")]
    Parse {
        /// Path whose TOML failed to parse into the target type.
        path: PathBuf,
        /// Underlying TOML error (boxed to keep the enum small).
        #[source]
        source: Box<toml::de::Error>,
    },
}

impl DiagnosticCode for TraceReadError {
    fn code(&self) -> &'static str {
        match self {
            TraceReadError::Read { .. } => "TRACE_READ_FAILED",
            TraceReadError::Parse { .. } => "TRACE_PARSE_FAILED",
        }
    }

    fn severity(&self) -> Severity {
        Severity::Error
    }

    fn location(&self) -> Option<Location> {
        let path = match self {
            TraceReadError::Read { path, .. } | TraceReadError::Parse { path, .. } => path.clone(),
        };
        Some(Location {
            file: Some(path),
            ..Location::default()
        })
    }
}

/// Parse a TOML file into the given type.
pub fn read_toml<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, TraceReadError> {
    let txt = fs::read_to_string(path).map_err(|source| TraceReadError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    let v = toml::from_str(&txt).map_err(|source| TraceReadError::Parse {
        path: path.to_path_buf(),
        source: Box::new(source),
    })?;
    Ok(v)
}

/// Parsed trace files from a single trace root.
#[derive(Debug)]
pub struct TraceFiles {
    /// Parsed `sys.toml` (empty-defaulted if missing). System-level
    /// requirements — the layer above HLR in the DO-178C §5.1 chain.
    /// Reuses the [`HlrFile`] shape by design: SYS and HLR share every
    /// field, the layer is signaled by filename.
    pub sys: HlrFile,
    /// Parsed `hlr.toml` (empty-defaulted if missing).
    pub hlr: HlrFile,
    /// Parsed `llr.toml` (empty-defaulted if missing).
    pub llr: LlrFile,
    /// Parsed `tests.toml` (empty-defaulted if missing).
    pub tests: TestsFile,
    /// Parsed `derived.toml`, or `None` if the file is absent.
    pub derived: Option<DerivedFile>,
}

/// Read all trace files from a root directory.
///
/// Missing files are returned with empty requirement lists and a warning is logged.
/// (`derived` returns `None` if absent.)
pub fn read_all_trace_files(root: &str) -> Result<TraceFiles, TraceReadError> {
    fn read_or_default<T: for<'de> Deserialize<'de>>(
        path: &Path,
        default: T,
    ) -> Result<T, TraceReadError> {
        if path.exists() {
            read_toml(path)
        } else {
            tracing::warn!(
                "Trace file not found: {} — using empty defaults. \
                 Check trace root path if this is unexpected.",
                path.display()
            );
            Ok(default)
        }
    }

    fn empty_hlr_file() -> HlrFile {
        HlrFile {
            meta: TraceMeta {
                document_id: "".into(),
                revision: "".into(),
            },
            schema: Schema { version: "".into() },
            requirements: vec![],
        }
    }

    let root_path = Path::new(root);

    if !root_path.exists() {
        tracing::warn!(
            "Trace root directory does not exist: {} — all trace files will be empty.",
            root_path.display()
        );
    }
    let sys = read_or_default(&root_path.join("sys.toml"), empty_hlr_file())?;
    let hlr = read_or_default(&root_path.join("hlr.toml"), empty_hlr_file())?;
    let llr = read_or_default(
        &root_path.join("llr.toml"),
        LlrFile {
            schema: Schema { version: "".into() },
            meta: TraceMeta {
                document_id: "".into(),
                revision: "".into(),
            },
            requirements: vec![],
        },
    )?;
    let tests = read_or_default(
        &root_path.join("tests.toml"),
        TestsFile {
            schema: Schema { version: "".into() },
            meta: TraceMeta {
                document_id: "".into(),
                revision: "".into(),
            },
            tests: vec![],
        },
    )?;

    let derived_path = root_path.join("derived.toml");
    let derived = if derived_path.exists() {
        Some(read_toml::<DerivedFile>(&derived_path)?)
    } else {
        None
    };

    Ok(TraceFiles {
        sys,
        hlr,
        llr,
        tests,
        derived,
    })
}
