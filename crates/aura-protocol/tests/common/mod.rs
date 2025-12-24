//! Shared verification infrastructure for consensus tests.
//!
//! This module contains test harnesses for:
//! - ITF trace conformance (Quint model checking)
//! - Reference implementation comparison (Lean proofs)
//! - Divergence reporting and diagnostics
//!
//! # Usage
//!
//! Integration tests can import this module:
//!
//! ```ignore
//! mod common;
//! use common::{itf_loader, divergence, reference};
//! ```

pub mod divergence;
pub mod itf_loader;
pub mod reference;
