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

pub mod types;
pub mod events;
pub mod state;
pub mod apply_event;
pub mod ledger;
pub mod serialization;

pub use types::*;
pub use events::*;
// Re-export the state current_timestamp to avoid ambiguity
pub use state::{AccountState, current_timestamp as state_current_timestamp};
pub use ledger::*;
pub use serialization::*;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum LedgerError {
    #[error("Invalid event: {0}")]
    InvalidEvent(String),
    
    #[error("Invalid signature")]
    InvalidSignature,
    
    #[error("Threshold not met: {current} < {required}")]
    ThresholdNotMet { current: usize, required: usize },
    
    #[error("Device not found: {0}")]
    DeviceNotFound(String),
    
    #[error("Guardian not found: {0}")]
    GuardianNotFound(String),
    
    #[error("Stale epoch: {provided} < {current}")]
    StaleEpoch { provided: u64, current: u64 },
    
    #[error("CRDT error: {0}")]
    CrdtError(String),
    
    #[error("Serialization error: {0}")]
    SerializationError(String),
    
    #[error("Serialization failed: {0}")]
    SerializationFailed(String),
    
    #[error("Automerge error: {0}")]
    AutomergeError(String),
}

pub type Result<T> = std::result::Result<T, LedgerError>;

