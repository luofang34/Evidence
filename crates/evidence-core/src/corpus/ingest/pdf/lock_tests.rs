//! Tool-lock schema and executable-verification tests (TEST-195).

use std::collections::BTreeMap;

use super::*;

/// A valid lock over the current platform with the pinned argv.
fn valid_lock() -> PdfToolLock {
    let platform = PdfPlatform::current().expect("supported test platform");
    let argv: Vec<String> = PINNED_ARGV.iter().map(|arg| (*arg).to_string()).collect();
    PdfToolLock {
        tool_name: PDF_TOOL_NAME.to_string(),
        version_output: "pdftotext version 25.10.0".to_string(),
        executable_sha256: BTreeMap::from([(platform, "a".repeat(64))]),
        config_digest: PdfToolLock::compute_config_digest(&argv, "1"),
        argv,
        adapter_version: "1".to_string(),
    }
}

/// Render a lock to its TOML wire form.
fn to_toml(lock: &PdfToolLock) -> String {
    toml::to_string(lock).expect("lock serializes")
}

#[test]
fn tool_lock_schema_rejects_unknown_fields_and_unpinned() {
    let valid = valid_lock();
    PdfToolLock::from_toml(&to_toml(&valid)).expect("valid lock parses");

    // Unknown fields fail closed.
    let with_unknown = format!("{}\nunknown_field = true\n", to_toml(&valid));
    assert!(matches!(
        PdfToolLock::from_toml(&with_unknown),
        Err(PdfToolLockError::RecordParse { .. })
    ));

    // Unsupported platform keys fail closed.
    let bad_platform = to_toml(&valid).replace("macos_aarch64", "plan9_pdp11");
    assert!(matches!(
        PdfToolLock::from_toml(&bad_platform),
        Err(PdfToolLockError::RecordParse { .. })
    ));

    // An empty executable map is an unpinned executable.
    let mut unpinned = valid.clone();
    unpinned.executable_sha256.clear();
    assert!(matches!(
        PdfToolLock::from_toml(&to_toml(&unpinned)),
        Err(PdfToolLockError::UnpinnedExecutable)
    ));

    // Malformed digests fail closed.
    let mut malformed = valid.clone();
    let platform = PdfPlatform::current().expect("supported test platform");
    malformed
        .executable_sha256
        .insert(platform, "not-hex".to_string());
    assert!(matches!(
        PdfToolLock::from_toml(&to_toml(&malformed)),
        Err(PdfToolLockError::MalformedDigest { .. })
    ));

    // A wrong tool name fails closed.
    let mut wrong_tool = valid.clone();
    wrong_tool.tool_name = "mutool".to_string();
    assert!(matches!(
        PdfToolLock::from_toml(&to_toml(&wrong_tool)),
        Err(PdfToolLockError::UnsupportedTool { .. })
    ));

    // Any argv deviation is a recipe change.
    let mut wrong_argv = valid.clone();
    wrong_argv.argv.push("-nodrm".to_string());
    assert!(matches!(
        PdfToolLock::from_toml(&to_toml(&wrong_argv)),
        Err(PdfToolLockError::ArgvMismatch { .. })
    ));

    // A stale configuration digest fails closed.
    let mut stale_config = valid.clone();
    stale_config.adapter_version = "2".to_string();
    assert!(matches!(
        PdfToolLock::from_toml(&to_toml(&stale_config)),
        Err(PdfToolLockError::ConfigDigestMismatch { .. })
    ));
}

#[cfg(unix)]
#[test]
fn verify_executable_rejects_digest_and_version_mismatch() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().expect("tempdir");
    let exe = dir.path().join("pdftotext-fake");
    let script = "#!/bin/sh\nprintf '%s\\n' 'pdftotext version 25.10.0' >&2\n";
    std::fs::write(&exe, script).expect("write fake");
    let mut permissions = std::fs::metadata(&exe).expect("metadata").permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&exe, permissions).expect("chmod");

    // The honest lock verifies the fake end to end.
    let mut lock = valid_lock();
    let digest = crate::hash::sha256(&std::fs::read(&exe).expect("read fake"));
    let platform = PdfPlatform::current().expect("supported test platform");
    lock.executable_sha256.insert(platform, digest.clone());
    lock.verify_executable(&exe).expect("honest fake verifies");

    // A digest mismatch fails closed with both values.
    let mut bad_digest = lock.clone();
    bad_digest
        .executable_sha256
        .insert(platform, "b".repeat(64));
    assert!(matches!(
        bad_digest.verify_executable(&exe),
        Err(PdfToolLockError::ExecutableDigestMismatch { .. })
    ));

    // A version-output mismatch fails closed with both values.
    let mut bad_version = lock.clone();
    bad_version.version_output = "pdftotext version 0.0.0".to_string();
    assert!(matches!(
        bad_version.verify_executable(&exe),
        Err(PdfToolLockError::VersionMismatch { .. })
    ));

    // A platform missing from the lock is unsupported.
    let mut missing_platform = lock.clone();
    missing_platform.executable_sha256 =
        BTreeMap::from([(PdfPlatform::LinuxX86_64, digest.clone())]);
    if platform != PdfPlatform::LinuxX86_64 {
        assert!(matches!(
            missing_platform.verify_executable(&exe),
            Err(PdfToolLockError::UnsupportedPlatform { .. })
        ));
    }
}
