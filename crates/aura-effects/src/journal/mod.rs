//! Journal handlers for testing
//!
//! This module provides simple implementations of JournalEffects for testing.
//! Complex journal handlers with OpLog, TreeState, etc. belong in aura-protocol
//! as they involve coordination and complex state management.

pub mod memory;

pub use memory::MemoryJournalHandler;
