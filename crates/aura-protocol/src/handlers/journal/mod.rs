//! Journal handlers for tree operations
//!
//! This module provides protocol-specific journal handlers that add
//! coordination features like capability guards on top of basic
//! journal handlers from aura-effects.

// pub mod guarded; // REMOVED: Uses deprecated CapabilityMiddleware

// pub use guarded::{GuardedJournalHandlerFactory, ProtocolContext, ProtocolJournalHandler}; // REMOVED

// Re-export basic journal handler from aura-effects
pub use aura_effects::journal::MemoryJournalHandler;
