//! CRDT-based authenticated ledger for account state
//!
//! This crate implements an eventually-consistent,
//! verifiable account ledger using CRDTs.
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

pub mod error;

/// Capability-based authorization system
pub mod capability;
/// Core ledger state machine implementation
/// - AccountState: The CRDT state structure
/// - AccountLedger: High-level validation and event log wrapper
/// - Event application: State transition logic and dispatch
pub mod core;
/// Protocol definitions and bootstrap
/// - Event types: All protocol event definitions
/// - Bootstrap: Account initialization and genesis ceremony
pub mod protocols;
/// Serialization utilities for CRDT events
pub mod serialization;
/// Type definitions for journal system
pub mod types;
/// Utility functions
pub mod utils;

// Session types disabled for now - placeholders for future implementation
// pub mod session_types;

// Re-export from protocols (events and bootstrap)
pub use protocols::*;
// Re-export from types
pub use types::*;
// Re-export from error
pub use error::*;
// Re-export from core (state and ledger)
pub use core::{AccountLedger, AccountState, Appliable};
// Re-export from serialization
pub use serialization::*;

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
    #[error("Invalid signature: {0}")]
    InvalidSignature(String),

    /// Threshold signature requirement not met
    #[error("Threshold not met: {current} < {required}")]
    ThresholdNotMet {
        /// Current number of signatures
        current: usize,
        /// Required number of signatures
        required: usize,
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
        current: u64,
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

    /// Weak cryptographic key
    #[error("Weak key: {0}")]
    WeakKey(String),

    /// Compromised cryptographic key
    #[error("Compromised key: {0}")]
    CompromisedKey(String),

    /// Key mismatch error
    #[error("Key mismatch: {0}")]
    KeyMismatch(String),

    /// Guardian revoked
    #[error("Guardian revoked: {0}")]
    GuardianRevoked(String),

    /// Guardian expired
    #[error("Guardian expired: {0}")]
    GuardianExpired(String),

    /// Guardian suspended
    #[error("Guardian suspended: {0}")]
    GuardianSuspended(String),

    /// Guardian inactive
    #[error("Guardian inactive: {0}")]
    GuardianInactive(String),

    /// Invalid public key
    #[error("Invalid public key: {0}")]
    InvalidPublicKey(String),

    /// Key rotation required
    #[error("Key rotation required: {0}")]
    KeyRotationRequired(String),

    /// Timestamp error
    #[error("Timestamp error: {0}")]
    TimestampError(String),

    /// Insufficient signers
    #[error("Insufficient signers: {0}")]
    InsufficientSigners(String),
}

/// Result type for ledger operations
pub type Result<T> = std::result::Result<T, LedgerError>;

impl From<capability::CapabilityError> for LedgerError {
    fn from(err: capability::CapabilityError) -> Self {
        LedgerError::CapabilityError(err.to_string())
    }
}
