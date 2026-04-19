//! CLI surface for `cargo evidence`.
//!
//! Every subcommand lives in its own module here. `main.rs` does only
//! argument parsing and dispatch; everything user-visible (text vs
//! JSON, warnings, exit codes) belongs in these modules.
//!
//! All business logic (git state, boundary loading, DAL resolution,
//! schema validation) lives in the `evidence` library and is called
//! through its public API. This module is thin by design — anything
//! that feels like "the tool's real behavior" belongs one layer down
//! so the `evidence` crate can be consumed by any downstream binary
//! without reaching into the CLI.

pub mod args;
pub mod check;
pub mod diff;
pub mod generate;
pub mod init;
pub mod output;
pub mod rules;
pub mod schema;
pub mod trace;
pub mod verify;
