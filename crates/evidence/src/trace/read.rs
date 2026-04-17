//! TOML reading helpers for trace files.
//!
//! Missing files are tolerated with empty defaults + a warning, so a
//! boundary config that points at an empty trace root still produces
//! a readable `TraceFiles` rather than a bail. The `backfill_uuids`
//! and validation passes handle the empty case gracefully.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::fs;
use std::path::Path;

use super::entries::{DerivedFile, HlrFile, LlrFile, Schema, TestsFile, TraceMeta};

/// Parse a TOML file into the given type.
pub fn read_toml<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    let txt = fs::read_to_string(path).with_context(|| format!("Reading {:?}", path))?;
    let v = toml::from_str(&txt).with_context(|| format!("Parsing {:?}", path))?;
    Ok(v)
}

/// Parsed trace files from a single trace root.
#[derive(Debug)]
pub struct TraceFiles {
    pub hlr: HlrFile,
    pub llr: LlrFile,
    pub tests: TestsFile,
    pub derived: Option<DerivedFile>,
}

/// Read all trace files from a root directory.
///
/// Missing files are returned with empty requirement lists and a warning is logged.
/// (`derived` returns `None` if absent.)
pub fn read_all_trace_files(root: &str) -> Result<TraceFiles> {
    fn read_or_default<T: for<'de> Deserialize<'de>>(path: &Path, default: T) -> Result<T> {
        if path.exists() {
            read_toml(path)
        } else {
            log::warn!(
                "Trace file not found: {} — using empty defaults. \
                 Check trace root path if this is unexpected.",
                path.display()
            );
            Ok(default)
        }
    }

    let root_path = Path::new(root);

    if !root_path.exists() {
        log::warn!(
            "Trace root directory does not exist: {} — all trace files will be empty.",
            root_path.display()
        );
    }
    let hlr = read_or_default(
        &root_path.join("hlr.toml"),
        HlrFile {
            meta: TraceMeta {
                document_id: "".into(),
                revision: "".into(),
            },
            schema: Schema { version: "".into() },
            requirements: vec![],
        },
    )?;
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
        hlr,
        llr,
        tests,
        derived,
    })
}
