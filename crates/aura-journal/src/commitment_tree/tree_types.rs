//! Tree types re-exported from aura-core
//!
//! This module provides a compatibility layer for tree types that are defined
//! in aura-core but commonly used in journal operations.
//!
//! # Architecture Note
//!
//! Tree types (AttestedOp, TreeOp, Policy, LeafNode, etc.) are foundational types
//! defined in Layer 1 (aura-core) because they are used by:
//! - Effect trait definitions (TreeEffects, SyncEffects)
//! - Cryptographic primitives (FROST tree signing)
//! - Authority abstraction
//!
//! This module re-exports them for convenience when working with commitment trees
//! in journal operations.

// Re-export all tree types from aura-core
pub use aura_core::tree::*;
