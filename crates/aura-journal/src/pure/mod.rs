//! Pure functions for Aeneas translation
//!
//! This module re-exports and documents the pure (no IO, no async) functions
//! from the journal implementation that are candidates for Aeneas translation
//! to Lean for formal verification.
//!
//! # Aeneas Compatibility
//!
//! Aeneas translates Rust code to Lean. For successful translation, functions must:
//! - Be pure (no IO, no async, no global state)
//! - Use types that Aeneas can handle
//! - Have minimal external dependencies
//!
//! # Current Status
//!
//! The functions exposed here wrap the actual implementations. Some may require
//! simplification or type adjustments for Aeneas to process them successfully.
//!
//! ## Aeneas-Ready Functions
//!
//! - `journal_join`: Journal merge via set union (semilattice join)
//!
//! ## Requires Simplification
//!
//! - `reduce_authority`: Complex reduction with many dependencies
//! - `reduce_context`: Complex reduction with many dependencies
//!
//! # Verification Strategy
//!
//! 1. **Differential testing** (current): Compare Lean oracle against Rust via JSON
//! 2. **Aeneas translation** (future): Translate these functions and prove properties

pub mod merge;
pub mod reduce;

pub use merge::*;
pub use reduce::*;
