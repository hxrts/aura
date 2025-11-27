//! Test and Mock Handlers
//!
//! This module contains test and mock handler implementations that were moved from
//! aura-protocol to maintain proper architectural boundaries. All test infrastructure
//! belongs in Layer 8 (aura-testkit), not in production layers.

pub mod memory;
pub mod mock;
pub mod tree;

// Re-export commonly used test handlers
pub use memory::{
    choreographic_memory::MemoryChoreographicHandler, effect_api_memory::MemoryLedgerHandler,
};
pub use mock::{MockCall, MockHandler};
pub use tree::dummy::DummyTreeHandler;
