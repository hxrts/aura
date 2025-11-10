//! FROST Threshold Signing Primitives
//!
//! This module provides pure cryptographic primitives for FROST threshold signatures
//! used in tree operations. It contains **NO** tree logic or business logic.

// Tree signing primitives
pub mod tree_signing;

// Re-export commonly used types
pub use tree_signing::*;
