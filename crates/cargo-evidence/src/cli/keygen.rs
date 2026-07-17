//! `cargo evidence keygen` — manage the project's ed25519 signing
//! keypair lifecycle.
//!
//! First-time use writes `cert/signing.key` (private; gitignored) and
//! `cert/signing.pub` (public; committed) and refuses if either file
//! already exists. `--rotate` is the sanctioned overwrite path: it
//! requires both files to exist, replaces them with a fresh keypair,
//! and appends one line to `cert/KEY-ROTATION-LOG` so the transition
//! is reviewable in git history.
//!
//! Refusing to silently regenerate is the design point. A "default
//! path → auto-generate-if-missing" UX would split the chain of
//! custody whenever a developer cloned without the private key
//! (which they should never have); the explicit `--rotate` boundary
//! pushes that case into a reviewer-visible commit.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};

use evidence_core::diagnostic::{Diagnostic, Location, Severity};
use evidence_core::{SigningKey, generate_signing_key, write_signing_key, write_verifying_key};

use super::args::{EXIT_ERROR, EXIT_SUCCESS, OutputFormat};
use super::output::{emit_json, emit_jsonl};

/// Default directory holding the keypair anchor and rotation log.
const DEFAULT_DIR: &str = "cert";
/// Filename of the private signing key (32-byte seed, hex-encoded).
const SIGNING_KEY_FILE: &str = "signing.key";
/// Filename of the public verifying key (32-byte point, hex-encoded).
const VERIFYING_KEY_FILE: &str = "signing.pub";
/// Append-only audit log of `--rotate` transitions.
const ROTATION_LOG_FILE: &str = "KEY-ROTATION-LOG";

/// Run the keygen subcommand.
pub fn cmd_keygen(
    rotate: bool,
    reason: Option<String>,
    out_dir: Option<PathBuf>,
    format: OutputFormat,
) -> Result<i32> {
    let dir = out_dir.unwrap_or_else(|| PathBuf::from(DEFAULT_DIR));
    let signing_path = dir.join(SIGNING_KEY_FILE);
    let verifying_path = dir.join(VERIFYING_KEY_FILE);

    let outcome = if rotate {
        let reason = reason.ok_or_else(|| {
            anyhow!("--rotate requires --reason <text> for the KEY-ROTATION-LOG entry")
        })?;
        rotate_keypair(&dir, &signing_path, &verifying_path, &reason)
    } else {
        create_keypair(&dir, &signing_path, &verifying_path)
    };

    emit_outcome(format, outcome, &signing_path, &verifying_path)
}

enum Outcome {
    Created(KeyMaterial),
    Rotated { new: KeyMaterial, log_line: String },
    RefusedExists { which: &'static str, path: PathBuf },
    RefusedRotateMissing { which: &'static str, path: PathBuf },
    Failed(KeygenError),
}

struct KeyMaterial {
    /// Hex of the public verifying key (64 chars).
    public_hex: String,
}

impl KeyMaterial {
    fn fingerprint(&self) -> &str {
        // First 16 hex chars (= 8 bytes of the public key) is more
        // than enough for human-readable identity at this scale.
        // The full public key is in `cert/signing.pub`.
        &self.public_hex[..16.min(self.public_hex.len())]
    }
}

#[derive(Debug, thiserror::Error)]
enum KeygenError {
    #[error("writing keypair: {0}")]
    Write(#[from] evidence_core::SigningError),
    #[error("appending rotation log line at {path}: {source}")]
    LogAppend {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("creating keypair directory at {path}: {source}")]
    Mkdir {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// Setting restrictive permissions on the private-key file
    /// failed. Unix-only — Windows / non-Unix targets do not
    /// distinguish "owner-only readable" at the POSIX-permission
    /// layer and the `chmod_private` stub there is a no-op.
    #[cfg(unix)]
    #[error("setting restrictive permissions on {path}: {source}")]
    Chmod {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

fn create_keypair(dir: &Path, signing_path: &Path, verifying_path: &Path) -> Outcome {
    if signing_path.exists() {
        return Outcome::RefusedExists {
            which: SIGNING_KEY_FILE,
            path: signing_path.to_path_buf(),
        };
    }
    if verifying_path.exists() {
        return Outcome::RefusedExists {
            which: VERIFYING_KEY_FILE,
            path: verifying_path.to_path_buf(),
        };
    }

    match write_fresh_pair(dir, signing_path, verifying_path) {
        Ok(material) => Outcome::Created(material),
        Err(e) => Outcome::Failed(e),
    }
}

fn rotate_keypair(dir: &Path, signing_path: &Path, verifying_path: &Path, reason: &str) -> Outcome {
    if !signing_path.exists() {
        return Outcome::RefusedRotateMissing {
            which: SIGNING_KEY_FILE,
            path: signing_path.to_path_buf(),
        };
    }
    if !verifying_path.exists() {
        return Outcome::RefusedRotateMissing {
            which: VERIFYING_KEY_FILE,
            path: verifying_path.to_path_buf(),
        };
    }

    let material = match write_fresh_pair(dir, signing_path, verifying_path) {
        Ok(m) => m,
        Err(e) => return Outcome::Failed(e),
    };

    let log_path = dir.join(ROTATION_LOG_FILE);
    let stamp = evidence_core::bundle::utc_now_rfc3339();
    let line = format!(
        "{stamp}  pubkey={}  reason={}\n",
        material.public_hex,
        reason.trim()
    );
    if let Err(source) = append_log(&log_path, &line) {
        return Outcome::Failed(KeygenError::LogAppend {
            path: log_path,
            source,
        });
    }

    Outcome::Rotated {
        new: material,
        log_line: line,
    }
}

fn write_fresh_pair(
    dir: &Path,
    signing_path: &Path,
    verifying_path: &Path,
) -> Result<KeyMaterial, KeygenError> {
    if !dir.as_os_str().is_empty() {
        fs::create_dir_all(dir).map_err(|source| KeygenError::Mkdir {
            path: dir.to_path_buf(),
            source,
        })?;
    }

    let key: SigningKey = generate_signing_key()?;
    write_signing_key(signing_path, &key)?;
    write_verifying_key(verifying_path, &key.verifying_key())?;
    chmod_private(signing_path)?;

    Ok(KeyMaterial {
        public_hex: hex::encode(key.verifying_key().to_bytes()),
    })
}

fn append_log(path: &Path, line: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut f = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    f.write_all(line.as_bytes())
}

#[cfg(unix)]
fn chmod_private(path: &Path) -> Result<(), KeygenError> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(path)
        .map_err(|source| KeygenError::Chmod {
            path: path.to_path_buf(),
            source,
        })?
        .permissions();
    perms.set_mode(0o600);
    fs::set_permissions(path, perms).map_err(|source| KeygenError::Chmod {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(not(unix))]
fn chmod_private(_path: &Path) -> Result<(), KeygenError> {
    // Windows / non-Unix: no portable POSIX-permission equivalent.
    // Document the gap in the user-facing message; users on these
    // platforms must restrict access via filesystem ACLs themselves.
    Ok(())
}

fn emit_outcome(
    format: OutputFormat,
    outcome: Outcome,
    signing_path: &Path,
    verifying_path: &Path,
) -> Result<i32> {
    match (format, outcome) {
        (OutputFormat::Jsonl, outcome) => emit_jsonl_outcome(outcome, signing_path, verifying_path),
        (OutputFormat::Json, outcome) => emit_json_outcome(outcome, signing_path, verifying_path),
        (OutputFormat::Human, outcome) => emit_human_outcome(outcome, signing_path, verifying_path),
    }
}

fn emit_human_outcome(outcome: Outcome, signing_path: &Path, verifying_path: &Path) -> Result<i32> {
    match outcome {
        Outcome::Created(m) => {
            println!("evidence: ed25519 keypair created");
            println!("  private (gitignore): {}", signing_path.display());
            println!("  public  (commit):    {}", verifying_path.display());
            println!("  fingerprint:         {}…", m.fingerprint());
            println!();
            println!("Next steps:");
            println!("  1. Verify '{}' is in .gitignore", signing_path.display());
            println!("  2. `git add {}` and commit", verifying_path.display());
            println!("  3. For CI signing, store the private key bytes as a repository secret;");
            println!("     `cargo evidence generate --signing-key <path>` reads it back.");
            Ok(EXIT_SUCCESS)
        }
        Outcome::Rotated { new, log_line } => {
            println!("evidence: ed25519 keypair rotated");
            println!("  private (overwritten):    {}", signing_path.display());
            println!("  public  (overwritten):    {}", verifying_path.display());
            println!("  new fingerprint:          {}…", new.fingerprint());
            println!("  rotation log line:        {}", log_line.trim_end());
            println!();
            println!("Next steps:");
            println!(
                "  1. `git add {}` and commit (the public key changed).",
                verifying_path.display()
            );
            println!("  2. Distribute the new public key to existing verifiers.");
            println!("  3. Re-sign any in-flight bundles whose verifiers cannot accept both keys.");
            Ok(EXIT_SUCCESS)
        }
        Outcome::RefusedExists { which, path } => {
            eprintln!("error: {} already exists at {}", which, path.display());
            eprintln!(
                "       to replace the existing keypair, run `cargo evidence keygen --rotate --reason <text>`"
            );
            eprintln!(
                "       (a fresh keygen never silently overwrites — the chain of custody depends on it)"
            );
            Ok(EXIT_ERROR)
        }
        Outcome::RefusedRotateMissing { which, path } => {
            eprintln!(
                "error: --rotate requires an existing keypair, but {} is missing at {}",
                which,
                path.display()
            );
            eprintln!("       drop the --rotate flag to create a first-time keypair");
            Ok(EXIT_ERROR)
        }
        Outcome::Failed(e) => Err(anyhow::Error::new(e).context("keygen failed")),
    }
}

fn emit_jsonl_outcome(outcome: Outcome, signing_path: &Path, verifying_path: &Path) -> Result<i32> {
    let (diag, terminal_code, exit) = outcome_to_diag(outcome, signing_path, verifying_path);
    if let Some(d) = diag {
        emit_jsonl(&d)?;
    }
    emit_jsonl(&Diagnostic {
        code: terminal_code.to_string(),
        severity: if terminal_code == "KEYGEN_OK" {
            Severity::Info
        } else {
            Severity::Error
        },
        message: terminal_message(terminal_code).to_string(),
        location: None,
        fix_hint: None,
        subcommand: Some("keygen".to_string()),
        root_cause_uid: None,
    })?;
    Ok(exit)
}

fn emit_json_outcome(outcome: Outcome, signing_path: &Path, verifying_path: &Path) -> Result<i32> {
    use serde_json::json;
    let (diag, terminal_code, exit) = outcome_to_diag(outcome, signing_path, verifying_path);
    let body = json!({
        "success": exit == EXIT_SUCCESS,
        "terminal": terminal_code,
        "diagnostic": diag,
    });
    emit_json(&body)?;
    Ok(exit)
}

fn outcome_to_diag(
    outcome: Outcome,
    signing_path: &Path,
    verifying_path: &Path,
) -> (Option<Diagnostic>, &'static str, i32) {
    match outcome {
        Outcome::Created(m) => (
            Some(Diagnostic {
                code: "KEYGEN_OK".to_string(),
                severity: Severity::Info,
                message: format!(
                    "ed25519 keypair created at {} / {} (fingerprint {}…)",
                    signing_path.display(),
                    verifying_path.display(),
                    m.fingerprint()
                ),
                location: None,
                fix_hint: None,
                subcommand: Some("keygen".to_string()),
                root_cause_uid: None,
            }),
            "KEYGEN_OK",
            EXIT_SUCCESS,
        ),
        Outcome::Rotated { new, log_line } => (
            Some(Diagnostic {
                code: "KEYGEN_OK".to_string(),
                severity: Severity::Info,
                message: format!(
                    "ed25519 keypair rotated; new fingerprint {}… (log: {})",
                    new.fingerprint(),
                    log_line.trim_end()
                ),
                location: None,
                fix_hint: None,
                subcommand: Some("keygen".to_string()),
                root_cause_uid: None,
            }),
            "KEYGEN_OK",
            EXIT_SUCCESS,
        ),
        Outcome::RefusedExists { which, path } => (
            Some(Diagnostic {
                code: "KEYGEN_KEY_EXISTS".to_string(),
                severity: Severity::Error,
                message: format!(
                    "{} already exists; use --rotate to replace explicitly",
                    which
                ),
                location: Some(Location {
                    file: Some(path),
                    ..Location::default()
                }),
                fix_hint: None,
                subcommand: Some("keygen".to_string()),
                root_cause_uid: None,
            }),
            "KEYGEN_FAIL",
            EXIT_ERROR,
        ),
        Outcome::RefusedRotateMissing { which, path } => (
            Some(Diagnostic {
                code: "KEYGEN_KEY_EXISTS".to_string(),
                severity: Severity::Error,
                message: format!("--rotate requires existing keypair; {} not found", which),
                location: Some(Location {
                    file: Some(path),
                    ..Location::default()
                }),
                fix_hint: None,
                subcommand: Some("keygen".to_string()),
                root_cause_uid: None,
            }),
            "KEYGEN_FAIL",
            EXIT_ERROR,
        ),
        Outcome::Failed(e) => {
            let err = anyhow::Error::new(e);
            (
                Some(Diagnostic {
                    code: "KEYGEN_FAIL".to_string(),
                    severity: Severity::Error,
                    message: format!("{:#}", err),
                    location: None,
                    fix_hint: None,
                    subcommand: Some("keygen".to_string()),
                    root_cause_uid: None,
                }),
                "KEYGEN_FAIL",
                EXIT_ERROR,
            )
        }
    }
}

fn terminal_message(code: &str) -> &str {
    match code {
        "KEYGEN_OK" => "keygen: keypair lifecycle step succeeded",
        "KEYGEN_FAIL" => "keygen: keypair lifecycle step failed",
        _ => "keygen: terminal",
    }
}

#[cfg(test)]
mod tests;
