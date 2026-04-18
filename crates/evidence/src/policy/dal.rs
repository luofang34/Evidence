//! `Dal` — DO-178C Design Assurance Level + per-crate `DalConfig`.
//!
//! `Dal::D` is the `Default` on purpose: a missing `[dal]` section in
//! `boundary.toml` must never silently *lower* cert requirements
//! below what was intended. A crate that inherits D (the least
//! stringent) on a misconfigured run is obvious; a crate that
//! inherits A on a misconfigured run would silently demand evidence
//! nobody knows to produce. The `#[derive(Ord)]` sort order (D < C <
//! B < A) matches this — later variants are *more* stringent, so
//! `max()` over a `dal_map` gives the highest required rigor.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::diagnostic::{DiagnosticCode, Severity};

/// Design Assurance Level per DO-178C.
/// A is most stringent, D is least. Default is D (safest: missing config
/// never accidentally lowers requirements below what was intended).
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub enum Dal {
    /// Lowest-rigor DO-178C level; default when no boundary config
    /// is present so an empty config never downgrades cert requirements.
    #[default]
    D,
    /// DO-178C Level C.
    C,
    /// DO-178C Level B.
    B,
    /// Highest-rigor DO-178C level — most objectives required with
    /// independence.
    A,
}

impl std::fmt::Display for Dal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Dal::A => write!(f, "A"),
            Dal::B => write!(f, "B"),
            Dal::C => write!(f, "C"),
            Dal::D => write!(f, "D"),
        }
    }
}

/// Error type for parsing a [`Dal`] from a string.
#[derive(Debug, thiserror::Error)]
pub enum ParseDalError {
    /// Input didn't match any of `A` / `B` / `C` / `D` (case-insensitive).
    #[error("unknown DAL '{0}'; expected one of: A, B, C, D")]
    Unknown(String),
}

impl DiagnosticCode for ParseDalError {
    fn code(&self) -> &'static str {
        match self {
            ParseDalError::Unknown(_) => "POLICY_UNKNOWN_DAL",
        }
    }

    fn severity(&self) -> Severity {
        Severity::Error
    }
}

impl std::str::FromStr for Dal {
    type Err = ParseDalError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_uppercase().as_str() {
            "A" => Ok(Dal::A),
            "B" => Ok(Dal::B),
            "C" => Ok(Dal::C),
            "D" => Ok(Dal::D),
            _ => Err(ParseDalError::Unknown(s.to_string())),
        }
    }
}

/// DAL configuration section in boundary.toml.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct DalConfig {
    /// Default DAL for all in-scope crates without explicit override.
    #[serde(default)]
    pub default_dal: Dal,
    /// Per-crate DAL overrides. Key is crate name.
    #[serde(default)]
    pub crate_overrides: BTreeMap<String, Dal>,
}

impl Default for DalConfig {
    fn default() -> Self {
        Self {
            default_dal: Dal::D,
            crate_overrides: BTreeMap::new(),
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
mod tests {
    use super::*;

    #[test]
    fn test_dal_display_parse() {
        for dal in [Dal::A, Dal::B, Dal::C, Dal::D] {
            let s = dal.to_string();
            let parsed: Dal = s.parse().unwrap();
            assert_eq!(parsed, dal);
        }
        assert!("E".parse::<Dal>().is_err());
        assert!("".parse::<Dal>().is_err());
    }

    #[test]
    fn test_dal_ordering() {
        assert!(Dal::A > Dal::B);
        assert!(Dal::B > Dal::C);
        assert!(Dal::C > Dal::D);
    }

    #[test]
    fn test_dal_default_is_d() {
        assert_eq!(Dal::default(), Dal::D);
    }

    #[test]
    fn test_dal_config_default() {
        let config = DalConfig::default();
        assert_eq!(config.default_dal, Dal::D);
        assert!(config.crate_overrides.is_empty());
    }
}
