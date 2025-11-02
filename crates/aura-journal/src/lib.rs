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
/// Simple session management for journal operations
pub mod session_types;
/// Type definitions for journal system
pub mod types;
/// Utility functions
pub mod utils;

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

/// Temporary macro for migrating current_timestamp() calls - will be removed
#[macro_export]
macro_rules! now {
    ($effects:expr) => {
        $effects.now().unwrap_or(0)
    };
}
