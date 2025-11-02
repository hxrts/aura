//! Backward compatibility module for in-memory transport
//!
//! This module provides compatibility shims for the old handler interface.
//! New code should use the unified transport system instead.

pub use super::{InMemoryHandler, NetworkHandler};
pub use crate::core::MemoryTransport as InMemoryTransport;
