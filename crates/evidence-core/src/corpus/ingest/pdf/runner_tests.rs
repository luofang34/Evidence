//! Blocking-runner contract tests (TEST-196): typed spawn,
//! exit-code, timeout, signal, missing-output, oversized-output,
//! and cleanup context, with no PATH lookup and no input
//! mutation. The fakes are shell scripts, so the behavioral cases
//! are Unix-only; the bare-name PATH guard is cross-platform.

use std::time::Duration;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use super::*;

/// Bounds keeping the failure cases fast: a generous wall clock
/// for the happy paths (parallel test load can delay spawn), so
/// only the dedicated timeout case uses a tight bound.
fn test_bounds() -> PdfRunBounds {
    PdfRunBounds {
        timeout: Duration::from_secs(20),
        max_stdout_bytes: 64 * 1024,
        max_stderr_bytes: 64 * 1024,
        max_output_bytes: 64 * 1024,
    }
}

/// A bare executable name is a PATH lookup: rejected before any
/// spawn, on every platform.
#[test]
fn bare_executable_name_is_a_forbidden_path_lookup() {
    let error = run_pdftotext_blocking(
        Path::new("pdftotext"),
        Path::new("input.pdf"),
        &test_bounds(),
    )
    .expect_err("bare name must be rejected");
    assert!(matches!(error, PdfRunError::PathLookupForbidden { .. }));
}

#[cfg(unix)]
/// Write an executable fake script into a fresh tempdir,
/// returning the dir guard, the script path, and a dummy
/// input path.
fn fake(script: &str) -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let exe = dir.path().join("fake-pdftotext");
    std::fs::write(&exe, script).expect("write fake");
    let mut permissions = std::fs::metadata(&exe).expect("metadata").permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&exe, permissions).expect("chmod");
    let input = dir.path().join("input.pdf");
    std::fs::write(&input, b"%PDF-1.4 fake\n").expect("write input");
    (dir, exe, input)
}

/// A fake that writes a fixed output file at the last argv.
#[cfg(unix)]
const WRITER: &str = "#!/bin/sh\nfor last; do :; done\nprintf '<doc/>\\n' > \"$last\"\n";

#[cfg(unix)]
#[test]
fn runner_success_and_typed_failures() {
    // Success: bounded stdout/stderr captured, output bytes
    // and digest returned, input left byte-identical.
    let (dir, exe, input) = fake(WRITER);
    let before = std::fs::read(&input).expect("read input");
    let extraction =
        run_pdftotext_blocking(&exe, &input, &test_bounds()).expect("honest fake succeeds");
    assert_eq!(extraction.output_bytes, b"<doc/>\n");
    assert_eq!(
        extraction.output_digest.as_str(),
        crate::hash::sha256(b"<doc/>\n")
    );
    assert_eq!(std::fs::read(&input).expect("read input"), before);
    drop(dir);

    // Spawn failure: an explicit but missing executable.
    let (dir, _exe, input) = fake(WRITER);
    let missing = dir.path().join("does-not-exist");
    let error = run_pdftotext_blocking(&missing, &input, &test_bounds())
        .expect_err("missing executable fails");
    assert!(matches!(error, PdfRunError::Spawn { .. }));

    // A nonzero documented exit (Poppler's permission-related
    // exits included) is respected, never bypassed.
    let (_dir, exe, input) = fake("#!/bin/sh\necho no-permission >&2\nexit 3\n");
    let error =
        run_pdftotext_blocking(&exe, &input, &test_bounds()).expect_err("nonzero exit fails");
    assert!(matches!(error, PdfRunError::ExitCode { code: Some(3), .. }));

    // Termination by signal is its own typed context.
    let (_dir, exe, input) = fake("#!/bin/sh\nkill -TERM $$\n");
    let error =
        run_pdftotext_blocking(&exe, &input, &test_bounds()).expect_err("signal termination fails");
    assert!(matches!(error, PdfRunError::Signal { signal: 15 }));

    // A zero exit without the output file is missing output.
    let (_dir, exe, input) = fake("#!/bin/sh\nexit 0\n");
    let error =
        run_pdftotext_blocking(&exe, &input, &test_bounds()).expect_err("missing output fails");
    assert!(matches!(error, PdfRunError::MissingOutput { .. }));
}

#[cfg(unix)]
#[test]
fn runner_bounds_timeout_and_output_limits() {
    // Timeout: the child is killed and reaped.
    let (_dir, exe, input) = fake("#!/bin/sh\nsleep 60\n");
    let bounds = PdfRunBounds {
        timeout: Duration::from_millis(500),
        ..test_bounds()
    };
    let error = run_pdftotext_blocking(&exe, &input, &bounds).expect_err("slow child times out");
    assert!(matches!(error, PdfRunError::Timeout { .. }));

    // Output bytes beyond the bound fail closed.
    let script = "#!/bin/sh\nfor last; do :; done\nhead -c 100 /dev/zero > \"$last\"\n";
    let (_dir, exe, input) = fake(script);
    let bounds = PdfRunBounds {
        max_output_bytes: 16,
        ..test_bounds()
    };
    let error = run_pdftotext_blocking(&exe, &input, &bounds).expect_err("oversized output fails");
    assert!(matches!(
        error,
        PdfRunError::OversizedOutput {
            what: "output-bytes",
            ..
        }
    ));

    // More produced files than the single output fail closed.
    let script =
        "#!/bin/sh\nfor last; do :; done\n: > \"$last\"\n: > \"$(dirname \"$last\")/extra\"\n";
    let (_dir, exe, input) = fake(script);
    let error = run_pdftotext_blocking(&exe, &input, &test_bounds()).expect_err("extra file fails");
    assert!(matches!(
        error,
        PdfRunError::OversizedOutput {
            what: "file-count",
            ..
        }
    ));

    // A chatty child neither deadlocks the wait nor escapes
    // the stderr bound.
    let script = "#!/bin/sh\nfor last; do :; done\n: > \"$last\"\nhead -c 200000 /dev/zero | tr '\\0' 'e' >&2\n";
    let (_dir, exe, input) = fake(script);
    let bounds = PdfRunBounds {
        max_stderr_bytes: 1024,
        timeout: Duration::from_secs(10),
        ..test_bounds()
    };
    let error =
        run_pdftotext_blocking(&exe, &input, &bounds).expect_err("chatty child fails closed");
    assert!(matches!(
        error,
        PdfRunError::OversizedOutput {
            what: "stderr-bytes",
            ..
        }
    ));
}
