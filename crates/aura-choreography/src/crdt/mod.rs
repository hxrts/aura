//! Session-Type-Based CRDT Choreographies
//!
//! This module implements Aura's session-type approach to CRDT conflict resolution
//! as described in docs/402_crdt_types.md. Instead of traditional CRDT data structures,
//! we use choreographic programming with algebraic effects to express CRDT semantics.
//!
//! The approach combines:
//! - Multi-party session types for communication protocols
//! - Algebraic effects for delivery and ordering guarantees
//! - Generic handlers that enforce CRDT semantic laws
//! - Message types that carry precise operation payloads

pub mod message_types;
pub mod semantic_interfaces;

pub use message_types::*;
pub use semantic_interfaces::*;
