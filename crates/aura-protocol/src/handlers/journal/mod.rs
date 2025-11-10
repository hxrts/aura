//! Journal handlers for tree operations
//!
//! This module provides implementations of JournalEffects for different
//! execution modes (testing, production, etc.).

pub mod guarded;
pub mod memory;

pub use guarded::{GuardedJournalHandlerFactory, ProtocolContext, ProtocolJournalHandler};
pub use memory::MemoryJournalHandler;
