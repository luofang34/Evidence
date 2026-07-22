//! `cargo evidence init`.

mod agent_context;
mod templates;

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;

use evidence_core::diagnostic::{Diagnostic, Severity};

use self::agent_context::write_agent_context_files;
use super::args::{EXIT_SUCCESS, OutputFormat};
use super::output::emit_jsonl;

/// The ordered next-step sequence printed on the human path and
/// carried in the `INIT_ADOPTION_INCOMPLETE` jsonl message. Every
/// verb is spelled exactly as the CLI accepts it, so the sequence
/// is executable as printed (LLR-151).
const NEXT_STEPS: &[&str] = &[
    "Edit cert/boundary.toml — declare your in-scope crates (and [dal] when you target cert/record)",
    "Add real requirements to cert/trace/{sys,hlr,llr,tests}.toml (examples are commented in each file)",
    "Run: cargo evidence trace --backfill-uuids",
    "Run: cargo evidence trace --validate",
    "Run: cargo evidence doctor",
    "Run: cargo evidence generate --out-dir evidence",
];

/// The adoption-incomplete explanation shared by the human prose
/// and the `INIT_ADOPTION_INCOMPLETE` diagnostic: a fresh scaffold
/// holds zero live requirements, and the named diagnostics are the
/// intended pre-adoption signal, not a scaffold error.
const ADOPTION_NOTE: &str = "this scaffold is adoption-incomplete: cert/trace holds zero live \
     requirements, so `cargo evidence trace --validate` reports TRACE_EVIDENCE_EMPTY and \
     `cargo evidence doctor` reports DOCTOR_TRACE_NO_EVIDENCE until real requirements exist — \
     the intended pre-adoption signal, not a scaffold error. Example entries in each trace file \
     are commented out; placeholder content never enters a bundle";

/// `cargo evidence init` handler: scaffold a `cert/` layout —
/// boundary config, floors config, per-profile stubs, and the
/// five trace files — for a fresh project (LLR-150). Every trace
/// template carries an explicitly empty entry list with the
/// worked example present only as comment lines, so a fresh
/// scaffold parses against the current schemas immediately and no
/// placeholder content can enter an evidence bundle as a real
/// requirement.
///
/// Re-runs are idempotent (LLR-151): without `force`, a managed
/// file that already exists is preserved byte-for-byte (reported
/// as preserved, not rewritten) and missing files are written.
/// With `force`, exactly the managed template set
/// ([`templates::managed_templates`]) is rewritten; anything
/// outside that set — including evidence the user added under
/// `cert/` — is never touched in either mode.
///
/// When `agent_context` is true, also writes a starter root
/// `CLAUDE.md` and `.claude/settings.json` snippet (see
/// `write_agent_context_files`). Those files are preserve-always
/// under both modes — the agent-context emitter never clobbers
/// downstream-authored conventions.
///
/// The run names the resulting state itself: the jsonl stream
/// emits one `INIT_ADOPTION_INCOMPLETE` info diagnostic ahead of
/// the single `INIT_OK` terminal, and the human output prints the
/// same note plus the complete ordered next-step sequence
/// ([`NEXT_STEPS`]).
pub fn cmd_init(force: bool, agent_context: bool, format: OutputFormat) -> Result<i32> {
    let jsonl = format == OutputFormat::Jsonl;

    // Create the directories the managed set writes into.
    fs::create_dir_all("cert/profiles")?;
    fs::create_dir_all("cert/trace")?;

    let mut written = 0u64;
    let mut preserved = 0u64;

    for template in templates::managed_templates() {
        let path = PathBuf::from(template.path);
        if path.exists() && !force {
            emit_template_preserved(jsonl, &path)?;
            preserved += 1;
            continue;
        }
        fs::write(&path, template.content)?;
        emit_template_written(jsonl, &path)?;
        written += 1;
    }

    // Agent-context files live outside the managed `cert/` set and
    // are preserve-always, so they count separately — the
    // written/preserved pair below describes the managed set only.
    let mut agent_context_written = 0u64;
    if agent_context {
        agent_context_written = write_agent_context_files(Path::new("."), jsonl)?;
    }

    if jsonl {
        emit_jsonl(&adoption_incomplete_diagnostic())?;
        emit_jsonl(&init_terminal(&format!(
            "init wrote {written} template file(s) (+ {agent_context_written} agent-context), \
             preserved {preserved} existing; {ADOPTION_NOTE}"
        )))?;
    } else {
        println!(
            "\nInitialized evidence tracking in cert/ \
             ({written} written, {preserved} preserved)."
        );
        println!("\nNote: {ADOPTION_NOTE}.");
        println!("\nNext steps:");
        for (i, step) in NEXT_STEPS.iter().enumerate() {
            println!("  {}. {}", i + 1, step);
        }
    }

    Ok(EXIT_SUCCESS)
}

/// Emit the per-file "created" event for a managed template that
/// init just wrote: one `INIT_TEMPLATE_WRITTEN` info diagnostic on
/// the jsonl stream, or a `created:` line on the human path.
pub(crate) fn emit_template_written(jsonl: bool, path: &Path) -> Result<()> {
    if jsonl {
        emit_jsonl(&Diagnostic {
            code: "INIT_TEMPLATE_WRITTEN".to_string(),
            severity: Severity::Info,
            message: format!("created {}", path.display()),
            location: Some(evidence_core::Location {
                file: Some(path.to_path_buf()),
                ..evidence_core::Location::default()
            }),
            fix_hint: None,
            subcommand: Some("init".to_string()),
            root_cause_uid: None,
        })?;
    } else {
        println!("created: {:?}", path);
    }
    Ok(())
}

/// Report a managed template that already exists and is being
/// preserved (the no-`--force` re-run path). Human output lists
/// the file per the idempotency contract; the jsonl stream stays
/// silent per preserved file — `INIT_TEMPLATE_WRITTEN` means
/// "written", and the `INIT_OK` terminal carries the preserved
/// count for machine consumers.
fn emit_template_preserved(jsonl: bool, path: &Path) -> Result<()> {
    if !jsonl {
        println!("preserved: {:?} (already exists)", path);
    }
    Ok(())
}

/// The `INIT_ADOPTION_INCOMPLETE` info diagnostic: rides the
/// jsonl stream ahead of the `INIT_OK` terminal (a finding, not a
/// second terminal — Schema Rule 1) so an agent consumer learns
/// from the init run itself that the scaffold is pre-adoption and
/// which executable sequence advances it.
fn adoption_incomplete_diagnostic() -> Diagnostic {
    Diagnostic {
        code: "INIT_ADOPTION_INCOMPLETE".to_string(),
        severity: Severity::Info,
        message: format!(
            "{ADOPTION_NOTE}. Next steps: {}",
            NEXT_STEPS
                .iter()
                .enumerate()
                .map(|(i, s)| format!("{}) {}", i + 1, s))
                .collect::<Vec<_>>()
                .join("; ")
        ),
        location: None,
        fix_hint: None,
        subcommand: Some("init".to_string()),
        root_cause_uid: None,
    }
}

/// The single `INIT_OK` terminal closing every successful init
/// jsonl stream (Schema Rule 1).
fn init_terminal(message: &str) -> Diagnostic {
    Diagnostic {
        code: "INIT_OK".to_string(),
        severity: Severity::Info,
        message: message.to_string(),
        location: None,
        fix_hint: None,
        subcommand: Some("init".to_string()),
        root_cause_uid: None,
    }
}
