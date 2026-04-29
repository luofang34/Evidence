//! Unit tests for `evidence_core::verify::runtime_error`. Lives in a
//! sibling file pulled in via `#[path]` so the parent stays under
//! the workspace 500-line limit.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use std::io;
use std::path::PathBuf;

use super::VerifyRuntimeError;
use crate::diagnostic::DiagnosticCode;

/// TEST-072 selector: the `ReadVerifyKey` variant pins
/// `code()` and `location()` to the public contract — the
/// diagnostic surfaces `VERIFY_RUNTIME_READ_VERIFY_KEY` and
/// carries the user-supplied key path so an agent can route a
/// fix-hint to the right file.
#[test]
fn read_verify_key_code_and_location() {
    let path = PathBuf::from("/tmp/no/such/key");
    let err = VerifyRuntimeError::ReadVerifyKey {
        path: path.clone(),
        source: io::Error::new(io::ErrorKind::NotFound, "no such file"),
    };

    assert_eq!(err.code(), "VERIFY_RUNTIME_READ_VERIFY_KEY");

    let loc = err.location().expect("ReadVerifyKey carries a location");
    assert_eq!(loc.file.as_deref(), Some(path.as_path()));
}
