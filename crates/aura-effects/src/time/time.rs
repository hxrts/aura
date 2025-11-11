//! Time effects trait definitions
//!
//! This module re-exports the TimeEffects trait from aura-core to provide
//! a unified interface for time operations across the system.

// Re-export time traits and types from aura-core
pub use aura_core::effects::{TimeEffects, TimeError, TimeoutHandle, WakeCondition};
