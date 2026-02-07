//! Evidence - Build evidence and reproducibility verification library
//!
//! This library provides tools for capturing build environments,
//! generating reproducible build evidence, and verifying builds.

pub mod bundle;
pub mod env;
pub mod git;
pub mod hash;
pub mod policy;
pub mod trace;
pub mod traits;
pub mod verify;
