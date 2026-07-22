//! `ResolutionPolicy` — the dependency-resolution contract every
//! cargo subprocess in the evidence pipeline runs under (LLR-139).
//!
//! One enum, two states:
//!
//! - [`ResolutionPolicy::LockedOffline`] — `--locked --offline`:
//!   the dependency graph resolves only from the committed lockfile
//!   and the local cargo cache. This is the default and the only
//!   policy a cert/record claim may be produced under.
//! - [`ResolutionPolicy::OnlineOptIn`] — no resolution flags: cargo
//!   may reach the network. Reachable only through the development
//!   profile's explicit `--online` opt-in; the opt-in is recorded
//!   in the bundle and can never back a cert/record claim.
//!
//! Every subprocess site (metadata, build, test, coverage, and
//! auxiliary analysis) appends [`ResolutionPolicy::cargo_args`]
//! verbatim, so the pipeline shares one resolution behavior rather
//! than per-call-site drift.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::diagnostic::{DiagnosticCode, Severity};
use crate::policy::Profile;
use crate::util::CmdError;

/// Dependency-resolution policy applied uniformly to every cargo
/// invocation the evidence pipeline spawns.
///
/// Serialized snake_case so `index.json` records `locked_offline` /
/// `online_opt_in` verbatim (LLR-142).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionPolicy {
    /// `--locked --offline`: resolve only from the committed lockfile
    /// and the local cargo cache; never reach the network.
    #[default]
    LockedOffline,
    /// No resolution flags: cargo may resolve online. Development-only
    /// opt-in; recorded in the bundle; cannot back a cert/record claim.
    OnlineOptIn,
}

impl ResolutionPolicy {
    /// The safe default for library call sites that do not plumb a
    /// policy through: cert-grade resolution is always locked/offline.
    /// The online path exists only through the CLI's development
    /// `--online` opt-in.
    pub const LOCKED_OFFLINE: Self = Self::LockedOffline;

    /// Resolve the policy for a (profile, online-opt-in) pair.
    ///
    /// - dev + opt-in ⇒ [`Self::OnlineOptIn`]
    /// - any profile without the opt-in ⇒ [`Self::LockedOffline`]
    /// - cert/record + opt-in ⇒ [`ResolutionPolicyError::OnlineForbidden`]
    pub fn for_profile(
        profile: Profile,
        online_opt_in: bool,
    ) -> Result<Self, ResolutionPolicyError> {
        match (profile, online_opt_in) {
            (Profile::Dev, true) => Ok(Self::OnlineOptIn),
            (Profile::Dev, false) | (Profile::Cert | Profile::Record, false) => {
                Ok(Self::LockedOffline)
            }
            (Profile::Cert | Profile::Record, true) => {
                Err(ResolutionPolicyError::OnlineForbidden { profile })
            }
        }
    }

    /// Subprocess argv fragment for this policy, appended verbatim to
    /// every cargo invocation in the pipeline. `OnlineOptIn` renders
    /// empty — the absence of `--locked` / `--offline` IS the opt-in.
    pub fn cargo_args(&self) -> &'static [&'static str] {
        match self {
            Self::LockedOffline => &["--locked", "--offline"],
            Self::OnlineOptIn => &[],
        }
    }

    /// Wire label matching the serde form; used in diagnostics.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::LockedOffline => "locked_offline",
            Self::OnlineOptIn => "online_opt_in",
        }
    }

    /// Map a failed cargo invocation under this policy.
    ///
    /// `Ok` carries the precise locked-graph diagnostic when the policy
    /// is [`Self::LockedOffline`] and cargo exited non-zero: the policy
    /// makes the cause class unambiguous (missing cached dependency /
    /// index data, or lock drift), so no stderr heuristics are
    /// consulted. `Err` hands the original [`CmdError`] back unchanged —
    /// the online policy, launch failures, and non-UTF-8 output keep
    /// their existing codes.
    pub fn offline_failure(
        &self,
        cmd: &'static str,
        err: CmdError,
    ) -> Result<LockedGraphError, CmdError> {
        match (self, err) {
            (Self::LockedOffline, CmdError::NonZeroExit { status, .. }) => {
                Ok(LockedGraphError::Unavailable {
                    cmd,
                    status: status.to_string(),
                })
            }
            (_, err) => Err(err),
        }
    }
}

/// Errors resolving a [`ResolutionPolicy`] from a (profile, opt-in)
/// pair.
#[derive(Debug, Error)]
pub enum ResolutionPolicyError {
    /// `--online` was passed with a cert/record profile. Online
    /// resolution can never back a certification claim, so the run is
    /// refused before any bundle work begins.
    #[error(
        "profile '{profile}' forbids online dependency resolution: `--online` is a \
         development-mode opt-in and cannot produce a cert/record claim"
    )]
    OnlineForbidden {
        /// Profile that rejected the opt-in.
        profile: Profile,
    },
}

impl DiagnosticCode for ResolutionPolicyError {
    fn code(&self) -> &'static str {
        match self {
            ResolutionPolicyError::OnlineForbidden { .. } => "POLICY_ONLINE_RESOLUTION_FORBIDDEN",
        }
    }

    fn severity(&self) -> Severity {
        Severity::Error
    }
}

/// A cargo invocation failed under `--locked --offline` (LLR-140).
///
/// One failure class shared by every pipeline error enum that wraps a
/// cargo subprocess; each owning enum's variant forwards `code()` here
/// so the diagnostic vocabulary stays singular. The message names the
/// command, the policy, and the remediation — populate the local cargo
/// cache from an online environment first.
#[derive(Debug, Error)]
pub enum LockedGraphError {
    /// The `--locked --offline` cargo invocation exited non-zero.
    #[error(
        "BUNDLE_LOCKED_GRAPH_UNAVAILABLE: locked dependency graph unavailable offline — \
         `{cmd}` failed ({status}) under resolution policy `locked_offline` \
         (--locked --offline). Populate the local cargo cache in an online environment \
         first (`cargo fetch --locked`), then re-run"
    )]
    Unavailable {
        /// Which cargo invocation failed (e.g. `cargo metadata`).
        cmd: &'static str,
        /// The exit status cargo returned, rendered for context.
        status: String,
    },
}

impl DiagnosticCode for LockedGraphError {
    fn code(&self) -> &'static str {
        match self {
            LockedGraphError::Unavailable { .. } => "BUNDLE_LOCKED_GRAPH_UNAVAILABLE",
        }
    }

    fn severity(&self) -> Severity {
        Severity::Error
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

    /// TEST-156 (a): the full (profile × opt-in) truth table.
    #[test]
    fn for_profile_truth_table() {
        assert_eq!(
            ResolutionPolicy::for_profile(Profile::Dev, true).unwrap(),
            ResolutionPolicy::OnlineOptIn
        );
        assert_eq!(
            ResolutionPolicy::for_profile(Profile::Dev, false).unwrap(),
            ResolutionPolicy::LockedOffline
        );
        assert_eq!(
            ResolutionPolicy::for_profile(Profile::Cert, false).unwrap(),
            ResolutionPolicy::LockedOffline
        );
        assert_eq!(
            ResolutionPolicy::for_profile(Profile::Record, false).unwrap(),
            ResolutionPolicy::LockedOffline
        );
        for profile in [Profile::Cert, Profile::Record] {
            assert!(
                matches!(
                    ResolutionPolicy::for_profile(profile, true),
                    Err(ResolutionPolicyError::OnlineForbidden { .. })
                ),
                "{profile} + --online must be refused"
            );
        }
    }

    /// TEST-156 (b): argv rendering — locked/offline carries both
    /// flags; the opt-in carries none (the absence IS the opt-in).
    #[test]
    fn cargo_args_match_policy() {
        assert_eq!(
            ResolutionPolicy::LockedOffline.cargo_args(),
            &["--locked", "--offline"]
        );
        assert!(ResolutionPolicy::OnlineOptIn.cargo_args().is_empty());
        assert_eq!(ResolutionPolicy::LOCKED_OFFLINE.as_str(), "locked_offline");
        assert_eq!(ResolutionPolicy::OnlineOptIn.as_str(), "online_opt_in");
    }

    /// TEST-156 (c): the refusal carries the registered code.
    #[test]
    fn online_forbidden_error_carries_policy_code() {
        let err = ResolutionPolicy::for_profile(Profile::Cert, true).unwrap_err();
        assert_eq!(err.code(), "POLICY_ONLINE_RESOLUTION_FORBIDDEN");
        assert_eq!(err.severity(), Severity::Error);
    }

    /// TEST-157 (d): only a non-zero exit under `LockedOffline` maps
    /// to the locked-graph diagnostic; launch failures, non-UTF-8
    /// output, and anything under `OnlineOptIn` pass through.
    #[test]
    fn offline_failure_maps_only_locked_nonzero_exits() {
        let launch = CmdError::Launch {
            prog: "cargo".to_string(),
            args: vec!["metadata".to_string()],
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "missing"),
        };
        assert!(
            ResolutionPolicy::LockedOffline
                .offline_failure("cargo metadata", launch)
                .is_err(),
            "launch failures keep their existing code"
        );

        let online_exit = CmdError::NonZeroExit {
            prog: "cargo".to_string(),
            args: vec!["metadata".to_string()],
            status: {
                let mut cmd = std::process::Command::new("false");
                cmd.status().expect("run false")
            },
        };
        assert!(
            ResolutionPolicy::OnlineOptIn
                .offline_failure("cargo metadata", online_exit)
                .is_err(),
            "the online policy never maps to the locked-graph diagnostic"
        );

        let locked_exit = CmdError::NonZeroExit {
            prog: "cargo".to_string(),
            args: vec!["metadata".to_string()],
            status: {
                let mut cmd = std::process::Command::new("false");
                cmd.status().expect("run false")
            },
        };
        let mapped = ResolutionPolicy::LockedOffline
            .offline_failure("cargo metadata", locked_exit)
            .expect("locked + non-zero exit maps to LockedGraphError");
        assert_eq!(mapped.code(), "BUNDLE_LOCKED_GRAPH_UNAVAILABLE");
        let msg = mapped.to_string();
        assert!(msg.contains("cargo metadata"), "names the command: {msg}");
        assert!(msg.contains("cargo fetch --locked"), "names the fix: {msg}");
    }
}
