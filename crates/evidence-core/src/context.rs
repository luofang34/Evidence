//! Per-module agent-context surface.
//!
//! Builds a single [`ContextReport`] for any selector — file path,
//! workspace crate name, Rust module path, or the workspace overview
//! when no selector is supplied. The report carries the LLR-level
//! requirements governing the selector, their parent HLR / SYS
//! roll-up, the verifying tests, the diagnostic codes they own, the
//! per-crate floors slice, the boundary policy, and a pointer to
//! the nearest layered `CLAUDE.md`.
//!
//! Two entry points:
//!
//! - [`resolve_selector`] classifies the raw input (priority on
//!   ambiguity: File > Crate > Module).
//! - [`context_for`] composes the report from the resolved
//!   selector.
//!
//! The implementation is split across private sibling files to stay
//! under the workspace 500-line file cap. Sub-modules are private
//! because the only stable public surface is the two entry points
//! plus the [`ContextReport`] / [`ContextError`] types.

mod error;
mod lookup;
mod report;
mod resolver;

#[cfg(test)]
mod tests;

pub use error::ContextError;
pub use lookup::context_for;
pub use report::{
    BoundarySlice, ContextReport, ContextWarning, Conventions, FloorRow, ParentRef, RequirementRef,
    SelectorView, TestRef,
};
pub use resolver::{ResolvedSelector, resolve_selector};
