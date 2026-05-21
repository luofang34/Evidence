//! `const fn` constructor helpers used to populate the
//! [`RULES`](super::RULES) array. Split out of the parent `rules.rs`
//! facade to keep that file under the 500-line workspace limit.
//!
//! Each helper is a thin convenience: it sets the domain to the
//! right [`super::Domain`] variant and pre-fills the boolean fields
//! (`has_fix_hint`, `terminal`) so the call-site stays short. Adding
//! a new helper means (a) a new `pub(super) const fn` here and (b) a
//! call from `RULES`'s definition.

use crate::diagnostic::Severity;

use super::{Domain, RuleEntry};

pub(super) const fn r(code: &'static str, severity: Severity, domain: Domain) -> RuleEntry {
    RuleEntry {
        code,
        severity,
        domain,
        has_fix_hint: false,
        terminal: false,
    }
}

pub(super) const fn req(code: &'static str, severity: Severity) -> RuleEntry {
    RuleEntry {
        code,
        severity,
        domain: Domain::Req,
        has_fix_hint: false,
        terminal: false,
    }
}

pub(super) const fn req_gap(code: &'static str) -> RuleEntry {
    RuleEntry {
        code,
        severity: Severity::Error,
        domain: Domain::Req,
        has_fix_hint: true,
        terminal: false,
    }
}

pub(super) const fn cli(code: &'static str, severity: Severity) -> RuleEntry {
    RuleEntry {
        code,
        severity,
        domain: Domain::Cli,
        has_fix_hint: false,
        terminal: false,
    }
}

pub(super) const fn context(code: &'static str, severity: Severity) -> RuleEntry {
    RuleEntry {
        code,
        severity,
        domain: Domain::Context,
        has_fix_hint: false,
        terminal: false,
    }
}

pub(super) const fn floors(code: &'static str, severity: Severity) -> RuleEntry {
    RuleEntry {
        code,
        severity,
        domain: Domain::Floors,
        has_fix_hint: false,
        terminal: false,
    }
}

pub(super) const fn terminal(code: &'static str, severity: Severity) -> RuleEntry {
    RuleEntry {
        code,
        severity,
        domain: match Domain::from_code_const(code) {
            Some(d) => d,
            None => Domain::Cli,
        },
        has_fix_hint: false,
        terminal: true,
    }
}
