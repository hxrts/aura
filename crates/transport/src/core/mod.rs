//! Core Transport Layer
//!
//! This module contains the fundamental transport abstractions and implementations
//! that form the foundation of the Aura transport system.

pub mod factory;
pub mod transport;
pub mod transport_trait;
pub mod unified_transport;

// Re-export core types for easy access
pub use factory::*;
// Re-export the primary Transport trait (from transport.rs)
pub use transport::*;
// Re-export unified transport trait with specific name to avoid ambiguity
pub use transport_trait::{Transport as UnifiedTransport};
pub use unified_transport::*;