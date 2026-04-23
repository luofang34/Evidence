//! Shared helpers for the `mcp_surface` integration test file.
//! Split out via `#[path]` so the parent stays under the 500-line
//! workspace limit as new tool-surface tests accumulate.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    dead_code,
    reason = "test-only helpers; parent uses a subset per-case"
)]

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{Value, json};

/// Spawn the built `evidence-mcp` binary with piped stdio.
///
/// The MCP wrapper internally calls `cargo evidence <verb>`,
/// which cargo resolves via `$PATH` to the `cargo-evidence`
/// binary. During `cargo test`, we want that spawn to pick up
/// the *locally-built* `cargo-evidence` (in `target/<profile>/`)
/// rather than whatever version the developer has installed via
/// `cargo install` — the installed copy may be stale and is
/// irrelevant to this test. `assert_cmd::cargo::cargo_bin`
/// returns paths under `target/<profile>/`; its parent is the
/// right dir to prepend to `PATH`.
pub fn spawn_server_with_cwd(cwd: Option<&std::path::Path>) -> Child {
    let bin = assert_cmd::cargo::cargo_bin("evidence-mcp");
    assert!(
        bin.exists(),
        "evidence-mcp binary missing at {bin:?} — run `cargo build -p evidence-mcp` first"
    );
    let target_dir = bin
        .parent()
        .expect("evidence-mcp binary has a parent dir")
        .to_path_buf();
    // Construct the new PATH with platform-correct separator:
    // `:` on Unix, `;` on Windows. `std::env::join_paths` handles
    // that and rejects entries containing the separator (e.g. a
    // weirdly-named directory), which is the right failure mode
    // here — a malformed PATH is worth failing loud, not silent.
    let mut entries: Vec<std::path::PathBuf> = vec![target_dir];
    if let Some(existing) = std::env::var_os("PATH") {
        entries.extend(std::env::split_paths(&existing));
    }
    let new_path = std::env::join_paths(entries).expect("valid PATH entries");
    let mut cmd = Command::new(&bin);
    cmd.env("PATH", new_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(path) = cwd {
        cmd.current_dir(path);
    }
    cmd.spawn().expect("spawn evidence-mcp")
}

/// Drive a scripted MCP session. Writes each frame on its own
/// line (the stdio transport expects newline-delimited JSON),
/// then reads back `expect_responses` response lines and returns
/// them parsed.
pub fn session(frames: &[Value], expect_responses: usize) -> Vec<Value> {
    session_in(frames, expect_responses, None)
}

pub fn session_in(
    frames: &[Value],
    expect_responses: usize,
    cwd: Option<&std::path::Path>,
) -> Vec<Value> {
    let mut child = spawn_server_with_cwd(cwd);
    let mut stdin: ChildStdin = child.stdin.take().expect("stdin");
    let stdout: ChildStdout = child.stdout.take().expect("stdout");
    let mut reader = BufReader::new(stdout);

    for frame in frames {
        writeln!(stdin, "{}", serde_json::to_string(frame).expect("encode")).expect("write");
    }
    drop(stdin);

    let mut responses = Vec::with_capacity(expect_responses);
    for _ in 0..expect_responses {
        let mut line = String::new();
        let n = reader.read_line(&mut line).expect("read_line");
        if n == 0 {
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        responses.push(serde_json::from_str::<Value>(trimmed).expect("parse response"));
    }

    child.wait().ok();
    responses
}

pub fn init_frames() -> Vec<Value> {
    vec![
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "mcp-surface-test", "version": "0"}
            }
        }),
        json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
            "params": {}
        }),
    ]
}
