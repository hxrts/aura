//! Group messaging and key agreement using BeeKEM protocol
//!
//! This crate implements the BeeKEM protocol for Aura, providing:
//! - Concurrent TreeKEM variant optimized for CRDT environments
//! - Forward secrecy and post-compromise security
//! - Deterministic roster management from capability events
//! - Causal encryption with predecessor key inclusion
//!
//! # Core Concepts
//!
//! - **BeeKEM Protocol**: Concurrent TreeKEM with CRDT-native operations
//! - **Roster Management**: Deterministic member list from capability graph
//! - **Causal Encryption**: Application-layer encryption including predecessor keys
//! - **Post-Compromise Security**: Future access revocation when members removed
//!
//! # Security Properties
//!
//! - Forward secrecy: Past messages remain secure after key compromise
//! - Post-compromise security: Future messages secure after member removal
//! - Concurrent safety: Multiple simultaneous operations converge correctly
//! - Deterministic ordering: All nodes agree on operation sequence

#![allow(missing_docs)] // TODO: Add comprehensive documentation in future work

pub mod beekem;
pub mod encryption;
pub mod events;
pub mod roster;
pub mod state;
pub mod types;

pub use beekem::*;
pub use encryption::*;
pub use events::*;
pub use roster::*;
pub use state::*;
pub use types::*;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum CgkaError {
    #[error("Invalid roster: {0}")]
    InvalidRoster(String),

    #[error("Member not found: {0}")]
    MemberNotFound(String),

    #[error("Epoch mismatch: expected {expected}, got {actual}")]
    EpochMismatch { expected: u64, actual: u64 },

    #[error("Invalid operation: {0}")]
    InvalidOperation(String),

    #[error("Cryptographic error: {0}")]
    CryptographicError(String),

    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("Journal error: {0}")]
    JournalError(String),
}

pub type Result<T> = std::result::Result<T, CgkaError>;
