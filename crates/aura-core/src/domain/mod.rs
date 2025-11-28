//! Domain-Specific Logic Types
//!
//! Core types that implement domain-specific semantics: consensus prestates,
//! journal CRDTs, and content addressing.
//!
//! **Layer 1**: Type definitions and interfaces. Implementations live in domain crates.

pub mod consensus;
pub mod content;
pub mod journal;

// Re-export all public types for convenience
pub use consensus::{Prestate, PrestateBuilder};
pub use content::{ChunkId, ContentId, ContentSize, Hash32};
pub use journal::{AuthLevel, Cap, Fact, FactValue, Journal};
