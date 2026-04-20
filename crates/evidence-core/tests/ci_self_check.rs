//! Meta self-check: the committed `.github/workflows/ci.yml`
//! must carry the two enforcement flags on the `trace-self-validate`
//! job.
//!
//! A future edit that strips either flag would silently regress the
//! self-trace contract — SYS traceability would become advisory
//! again, and test-selector rot would become silent again. This
//! test fires before that change can merge.
//!
//! The check is grep-level on the committed YAML, not a parse. We
//! don't need to understand YAML structure to know whether two
//! specific flag strings appear under the `trace-self-validate`
//! job's `run:` block.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use std::fs;
use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    // This test lives at crates/evidence-core/tests/; workspace root is
    // two directories up.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

/// Locate the region of `ci.yml` belonging to the
/// `trace-self-validate` job and return it as a single string.
///
/// Naïve YAML scan: finds the line `trace-self-validate:` at any
/// indentation, then returns everything until the next top-level
/// job key (two-space indent at column 2) or EOF.
fn extract_trace_self_validate_job(yaml: &str) -> String {
    let mut in_job = false;
    let mut out = String::new();
    for line in yaml.lines() {
        if line.trim_start().starts_with("trace-self-validate:") {
            in_job = true;
            out.push_str(line);
            out.push('\n');
            continue;
        }
        if in_job {
            // Stop when we hit the next top-level job (two-space
            // indent, no deeper).
            let trimmed_start = line.trim_start();
            let indent = line.len() - trimmed_start.len();
            if indent == 2 && !trimmed_start.is_empty() && trimmed_start.ends_with(':') {
                break;
            }
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// TEST-024: Both enforcement flags must appear on the
/// `trace-self-validate` job. The job runs `cargo evidence trace
/// --validate …`; omitting `--require-hlr-sys-trace` or
/// `--check-test-selectors` would silently regress the self-trace
/// contract.
#[test]
fn ci_yaml_has_enforcement_flags() {
    let yaml_path = workspace_root().join(".github/workflows/ci.yml");
    let yaml = fs::read_to_string(&yaml_path).unwrap_or_else(|e| {
        panic!("reading {}: {}", yaml_path.display(), e);
    });

    let job = extract_trace_self_validate_job(&yaml);
    assert!(
        !job.is_empty(),
        "could not locate trace-self-validate job in {}",
        yaml_path.display()
    );

    for flag in [
        "--require-hlr-sys-trace",
        "--check-test-selectors",
        "--require-hlr-surface-bijection",
    ] {
        assert!(
            job.contains(flag),
            "trace-self-validate job is missing `{}` flag — the self-trace \
             contract regresses silently without it.\nJob block:\n{}",
            flag,
            job
        );
    }
}

/// Sanity-check the extractor so a test false-negative can't mask
/// a real regression: a YAML without the target job should return
/// an empty region.
#[test]
fn extractor_returns_empty_when_job_absent() {
    let yaml = r#"
jobs:
  check:
    name: Check
    runs-on: ubuntu-latest
"#;
    assert_eq!(extract_trace_self_validate_job(yaml), "");
}

/// And a YAML with an unrelated job after the target should end
/// the region at the next top-level key.
#[test]
fn extractor_stops_at_next_job() {
    let yaml = r#"jobs:
  trace-self-validate:
    steps:
      - run: --require-hlr-sys-trace --check-test-selectors
  next-job:
    steps:
      - run: do_something_else
"#;
    let region = extract_trace_self_validate_job(yaml);
    assert!(region.contains("--require-hlr-sys-trace"));
    assert!(!region.contains("do_something_else"));
}
