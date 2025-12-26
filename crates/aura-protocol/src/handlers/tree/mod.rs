//! Layer 4: Tree Handler Implementations
//!
//! Handlers for commitment tree operations.
//!
//! ## Handlers
//!
//! - **PersistentTreeHandler**: Production handler with storage persistence
//!
//! **Note**: Tree reduction and application logic lives in aura-journal (Layer 2),
//! enabling separation between domain CRDT operations and protocol-layer orchestration.

pub mod persistent;

pub use persistent::PersistentTreeHandler;
