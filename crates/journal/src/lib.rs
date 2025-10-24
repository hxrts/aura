//! CRDT-based authenticated ledger for account state
//!
//! This crate implements an eventually-consistent, verifiable account ledger
//! using Automerge CRDTs and threshold-signed events.
//!
//! # Core Concepts
//!
//! - **Events**: Threshold-signed operations (add device, add guardian, etc.)
//! - **Account State**: Current view of account configuration
//! - **CRDT Semantics**: Merge-able state for offline-first operation
//! - **Nonce Tracking**: Prevents replay attacks
//!
//! # Security Invariants
//!
//! - All high-impact events require M-of-N threshold signatures
//! - Nonces ensure events are applied exactly once
//! - Session epochs invalidate old presence tickets
//! - Device/guardian removal uses tombstones (G-Set CRDT)

#![allow(missing_docs)] // TODO: Add comprehensive documentation in future work

/// Event application logic and state transitions
pub mod apply_event;
/// Account bootstrap with capability-based authorization
pub mod bootstrap;
/// Capability-based authorization system
pub mod capability;
/// CRDT-based event definitions
pub mod events;
/// Main account ledger implementation
pub mod ledger;
/// Serialization utilities for CRDT events
pub mod serialization;
/// Account state management
pub mod state;
/// Type definitions for journal system
pub mod types;

pub use events::*;
pub use types::*;
// Re-export the state current_timestamp to avoid ambiguity
pub use ledger::*;
pub use serialization::*;
pub use state::AccountState;


use thiserror::Error;

/// Temporary macro for migrating current_timestamp() calls - will be removed
#[macro_export]
macro_rules! now {
    ($effects:expr) => {
        $effects.now().unwrap_or(0)
    };
}

/// Error types for ledger operations
#[derive(Error, Debug)]
pub enum LedgerError {
    /// Invalid event format or content
    #[error("Invalid event: {0}")]
    InvalidEvent(String),

    /// Invalid threshold signature
    #[error("Invalid signature")]
    InvalidSignature,

    /// Threshold signature requirement not met
    #[error("Threshold not met: {current} < {required}")]
    ThresholdNotMet { 
        /// Current number of signatures
        current: usize, 
        /// Required number of signatures
        required: usize 
    },

    /// Device not found in account
    #[error("Device not found: {0}")]
    DeviceNotFound(String),

    /// Guardian not found in account
    #[error("Guardian not found: {0}")]
    GuardianNotFound(String),

    /// Session epoch is stale
    #[error("Stale epoch: {provided} < {current}")]
    StaleEpoch { 
        /// Provided epoch number
        provided: u64, 
        /// Current epoch number
        current: u64 
    },

    /// CRDT operation failed
    #[error("CRDT error: {0}")]
    CrdtError(String),

    /// Serialization/deserialization failed
    #[error("Serialization error: {0}")]
    SerializationError(String),

    /// Serialization operation failed
    #[error("Serialization failed: {0}")]
    SerializationFailed(String),

    /// Capability authorization failed
    #[error("Capability error: {0}")]
    CapabilityError(String),

    /// Automerge CRDT operation failed
    #[error("Automerge error: {0}")]
    AutomergeError(String),
}

/// Result type for ledger operations
pub type Result<T> = std::result::Result<T, LedgerError>;

impl From<capability::CapabilityError> for LedgerError {
    fn from(err: capability::CapabilityError) -> Self {
        LedgerError::CapabilityError(err.to_string())
    }
}
