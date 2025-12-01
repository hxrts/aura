//! Layer 4: Tree Handler Implementations
//!
//! Handlers for commitment tree operations. Production handlers removed during refactor;
//! dummy handler remains for tests and composite handler composition.
//!
//! **Note**: Tree reduction and application logic now lives in aura-journal (Layer 2),
//! enabling separation between domain CRDT operations and protocol-layer orchestration.

pub mod dummy;
pub mod in_memory;

pub use dummy::DummyTreeHandler;
pub use in_memory::InMemoryTreeHandler;
