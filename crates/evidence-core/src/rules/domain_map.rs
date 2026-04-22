//! Prefix → [`Domain`] mapping, split from the `rules.rs` facade to
//! keep that file under the 500-line workspace limit.
//!
//! The mapping is the only place in the crate that tolerates adding
//! a new domain without touching every table simultaneously — adding
//! a new prefix here without a matching [`Domain`] variant fails to
//! compile, and adding a variant without a prefix leaves `from_code`
//! returning `None` (the bijection test in
//! `crates/evidence-core/tests/diagnostic_codes_locked/*.rs`
//! catches that).

use super::Domain;

impl Domain {
    /// `const fn` twin of [`Domain::from_code`] used inside the
    /// `terminal(…)` constructor.
    pub(super) const fn from_code_const(code: &str) -> Option<Self> {
        let bytes = code.as_bytes();
        let mut i = 0;
        while i < bytes.len() && bytes[i] != b'_' {
            i += 1;
        }
        let prefix = match std::str::from_utf8(bytes.split_at(i).0) {
            Ok(s) => s,
            Err(_) => return None,
        };
        match prefix.as_bytes() {
            b"BOUNDARY" => Some(Self::Boundary),
            b"BUNDLE" => Some(Self::Bundle),
            b"CHECK" => Some(Self::Check),
            b"CLI" => Some(Self::Cli),
            b"CMD" => Some(Self::Cmd),
            b"DOCTOR" => Some(Self::Doctor),
            b"ENV" => Some(Self::Env),
            b"FLOORS" => Some(Self::Floors),
            b"GENERATE" => Some(Self::Generate),
            b"GIT" => Some(Self::Git),
            b"INIT" => Some(Self::Init),
            b"HASH" => Some(Self::Hash),
            b"POLICY" => Some(Self::Policy),
            b"REQ" => Some(Self::Req),
            b"SCHEMA" => Some(Self::Schema),
            b"SIGN" => Some(Self::Sign),
            b"TESTS" => Some(Self::Tests),
            b"TRACE" => Some(Self::Trace),
            b"VERIFY" => Some(Self::Verify),
            _ => None,
        }
    }
}
