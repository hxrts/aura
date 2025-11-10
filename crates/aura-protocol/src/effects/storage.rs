//! Storage effects for key-value operations
//!
//! This module re-exports the StorageEffects trait from aura-core to provide
//! a unified interface for storage operations across the system.

// Re-export storage traits and types from aura-core
pub use aura_core::effects::{StorageEffects, StorageError, StorageLocation, StorageStats};
