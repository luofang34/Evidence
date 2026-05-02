//! Unit tests for the `cargo evidence keygen` lifecycle: create,
//! refuse-overwrite-without-rotate, --rotate semantics, log append,
//! and per-platform permission checks.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use std::fs;

use super::{ROTATION_LOG_FILE, cmd_keygen};
use crate::cli::args::{EXIT_ERROR, EXIT_SUCCESS, OutputFormat};

fn dir() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

#[test]
fn first_time_create_writes_both_files() {
    let tmp = dir();
    let exit = cmd_keygen(
        false,
        None,
        Some(tmp.path().to_path_buf()),
        OutputFormat::Human,
    )
    .unwrap();
    assert_eq!(exit, EXIT_SUCCESS);
    assert!(tmp.path().join("signing.key").exists());
    assert!(tmp.path().join("signing.pub").exists());
    // No rotation log on first creation.
    assert!(!tmp.path().join(ROTATION_LOG_FILE).exists());
}

#[test]
fn refuses_overwrite_without_rotate() {
    let tmp = dir();
    cmd_keygen(
        false,
        None,
        Some(tmp.path().to_path_buf()),
        OutputFormat::Human,
    )
    .unwrap();
    let exit = cmd_keygen(
        false,
        None,
        Some(tmp.path().to_path_buf()),
        OutputFormat::Human,
    )
    .unwrap();
    assert_eq!(
        exit, EXIT_ERROR,
        "second keygen without --rotate must refuse"
    );
}

#[test]
fn rotate_requires_existing_pair() {
    let tmp = dir();
    let exit = cmd_keygen(
        true,
        Some("test rotation".to_string()),
        Some(tmp.path().to_path_buf()),
        OutputFormat::Human,
    )
    .unwrap();
    assert_eq!(
        exit, EXIT_ERROR,
        "--rotate without existing pair must refuse"
    );
}

#[test]
fn rotate_overwrites_and_logs() {
    let tmp = dir();
    cmd_keygen(
        false,
        None,
        Some(tmp.path().to_path_buf()),
        OutputFormat::Human,
    )
    .unwrap();
    let pub_before = fs::read_to_string(tmp.path().join("signing.pub")).unwrap();

    let exit = cmd_keygen(
        true,
        Some("compromised host".to_string()),
        Some(tmp.path().to_path_buf()),
        OutputFormat::Human,
    )
    .unwrap();
    assert_eq!(exit, EXIT_SUCCESS);

    let pub_after = fs::read_to_string(tmp.path().join("signing.pub")).unwrap();
    assert_ne!(pub_before, pub_after, "public key must change on rotate");

    let log = fs::read_to_string(tmp.path().join(ROTATION_LOG_FILE)).unwrap();
    assert!(
        log.contains("compromised host"),
        "log must carry reason: {log}"
    );
    assert!(
        log.contains("pubkey="),
        "log must carry pubkey field: {log}"
    );
}

#[test]
fn rotate_without_reason_is_an_error() {
    let tmp = dir();
    cmd_keygen(
        false,
        None,
        Some(tmp.path().to_path_buf()),
        OutputFormat::Human,
    )
    .unwrap();
    let err = cmd_keygen(
        true,
        None,
        Some(tmp.path().to_path_buf()),
        OutputFormat::Human,
    )
    .unwrap_err();
    assert!(err.to_string().contains("--reason"), "got: {err:#}");
}

#[test]
fn rotate_appends_each_log_line() {
    let tmp = dir();
    cmd_keygen(
        false,
        None,
        Some(tmp.path().to_path_buf()),
        OutputFormat::Human,
    )
    .unwrap();
    cmd_keygen(
        true,
        Some("rotation #1".to_string()),
        Some(tmp.path().to_path_buf()),
        OutputFormat::Human,
    )
    .unwrap();
    cmd_keygen(
        true,
        Some("rotation #2".to_string()),
        Some(tmp.path().to_path_buf()),
        OutputFormat::Human,
    )
    .unwrap();

    let log = fs::read_to_string(tmp.path().join(ROTATION_LOG_FILE)).unwrap();
    let lines: Vec<&str> = log.lines().collect();
    assert_eq!(lines.len(), 2, "two rotations → two log lines: {log}");
    assert!(lines[0].contains("rotation #1"));
    assert!(lines[1].contains("rotation #2"));
}

#[cfg(unix)]
#[test]
fn signing_key_chmod_600_on_unix() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = dir();
    cmd_keygen(
        false,
        None,
        Some(tmp.path().to_path_buf()),
        OutputFormat::Human,
    )
    .unwrap();
    let perms = fs::metadata(tmp.path().join("signing.key"))
        .unwrap()
        .permissions();
    // 0o600 = owner-rw only. Mask off type bits.
    assert_eq!(perms.mode() & 0o777, 0o600);
}
