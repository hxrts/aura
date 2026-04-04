//! System Workflow - Portable Business Logic
//!
//! This module contains system-level operations that are portable across all frontends.

#![allow(missing_docs)]

pub mod hooks;
#[allow(dead_code)]
mod legacy;
pub mod refresh;
pub mod versioning;

pub use hooks::*;
pub use refresh::*;
pub use versioning::*;
