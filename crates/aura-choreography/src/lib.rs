//! Aura Choreographic Protocol Implementations
//!
//! This crate contains concrete implementations of Aura's distributed protocols
//! using choreographic programming patterns following docs/405_protocol_guide.md.
//! It builds on the unified effect system provided by `aura-protocol`.
//!
//! ## Architecture Overview
//!
//! Following the protocol guide layered architecture:
//! ```text
//! Session Type Algebra → Choreographic DSL → Effect System → Semilattice Types
//! ```
//!
//! ## Protocol Categories
//!
//! - **DKD Protocols**: Deterministic key derivation choreographies
//! - **FROST Protocols**: Threshold signature choreographies  
//! - **Consensus Protocols**: Agreement and coordination choreographies
//! - **Semilattice Protocols**: CRDT synchronization choreographies
//!
//! All protocols are implemented using the rumpsteak-aura `choreography!` macro for
//! compile-time verified session types and deadlock-free distributed execution.
//!
//! ## Quick Start
//!
//! ```rust,ignore
//! use aura_choreography::runtime::AuraHandlerAdapterFactory;
//!
//! // Create choreographic handler with unified effect system
//! let mut adapter = AuraHandlerAdapterFactory::for_testing(device_id);
//!
//! // Execute choreographic protocol using the protocol guide patterns
//! // See docs/405_protocol_guide.md for detailed examples
//! ```

#![allow(clippy::result_large_err)]
#![allow(clippy::large_enum_variant)]
#![allow(missing_docs, dead_code)]

/// Choreographic protocol definitions following docs/405_protocol_guide.md
pub mod protocols;

/// Semilattice CRDT choreographies
// pub mod semilattice;

/// Common utilities shared across protocols
pub mod common;

/// Rumpsteak-compatible type definitions for choreography generation
pub mod types;

/// Runtime infrastructure for executing rumpsteak-generated choreographies  
pub mod runtime;

/// Unified choreography adapters following docs/405_protocol_guide.md
pub mod integration;

// Re-export key types for convenience
pub use aura_protocol::effects::choreographic::ChoreographyError;

// Re-export protocol implementations following protocol guide structure
pub use protocols::*;
// pub use semilattice::*;
