//! Layer 4: Tree Handler Implementations
//!
//! Handlers for commitment tree operations.
//!
//! ## Handlers
//!
//! - **PersistentTreeHandler**: Production handler with storage persistence
//! - **InMemoryTreeHandler**: Legacy handler for testing (to be removed)
//! - **DummyTreeHandler**: No-op handler for composite handler composition
//!
//! **Note**: Tree reduction and application logic lives in aura-journal (Layer 2),
//! enabling separation between domain CRDT operations and protocol-layer orchestration.

pub mod dummy;
pub mod in_memory;
pub mod persistent;

pub use dummy::DummyTreeHandler;
pub use in_memory::InMemoryTreeHandler;
pub use persistent::PersistentTreeHandler;
