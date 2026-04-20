//! `Profile` — build/certification profile (dev / cert / record).
//!
//! Kept alongside its `FromStr` / `Display` / `ParseProfileError` so a
//! reader looking at how `"cert"` becomes a `Profile` sees the whole
//! dispatch in one place.

use serde::{Deserialize, Serialize};

use crate::diagnostic::{DiagnosticCode, Severity};

/// Build/certification profile (e.g., dev, cert, record).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Profile {
    /// Development profile - relaxed checks
    #[default]
    Dev,
    /// Certification profile - strict checks
    Cert,
    /// Recording profile - captures evidence without enforcement
    Record,
}

impl std::fmt::Display for Profile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Profile::Dev => write!(f, "dev"),
            Profile::Cert => write!(f, "cert"),
            Profile::Record => write!(f, "record"),
        }
    }
}

/// Error type for parsing a [`Profile`] from a string.
///
/// Typed error (via `thiserror`) instead of `anyhow::Error` so library
/// callers can match on the failure mode without string-grepping. The
/// CLI layer can still `?` it into an `anyhow::Error` if that's what
/// its error envelope wants.
#[derive(Debug, thiserror::Error)]
pub enum ParseProfileError {
    /// Input didn't match any of `dev` / `cert` / `record` (case-insensitive).
    #[error("unknown profile '{0}'; expected one of: dev, cert, record")]
    Unknown(String),
}

impl DiagnosticCode for ParseProfileError {
    fn code(&self) -> &'static str {
        match self {
            ParseProfileError::Unknown(_) => "POLICY_UNKNOWN_PROFILE",
        }
    }

    fn severity(&self) -> Severity {
        Severity::Error
    }
}

impl std::str::FromStr for Profile {
    type Err = ParseProfileError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "dev" => Ok(Profile::Dev),
            "cert" => Ok(Profile::Cert),
            "record" => Ok(Profile::Record),
            _ => Err(ParseProfileError::Unknown(s.to_string())),
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
    fn test_profile_display() {
        assert_eq!(Profile::Dev.to_string(), "dev");
        assert_eq!(Profile::Cert.to_string(), "cert");
        assert_eq!(Profile::Record.to_string(), "record");
    }

    #[test]
    fn test_profile_parse() {
        assert_eq!("dev".parse::<Profile>().unwrap(), Profile::Dev);
        assert_eq!("cert".parse::<Profile>().unwrap(), Profile::Cert);
        assert_eq!("CERT".parse::<Profile>().unwrap(), Profile::Cert);
        assert!("unknown".parse::<Profile>().is_err());
    }
}
