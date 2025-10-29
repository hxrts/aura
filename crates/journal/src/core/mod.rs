//! Core ledger state machine implementation
//!
//! This module contains the state machine engine for the account ledger:
//! - AccountState: The CRDT state structure
//! - AccountLedger: High-level validation and event log wrapper
//! - Event application: State transition logic and dispatch
//!
//! These components work together to implement the core ledger operations.

pub mod appliable;
pub mod apply_event;
pub mod ledger;
pub mod state;

pub use appliable::Appliable;
pub use ledger::{AccountLedger, CompactionProposal, CompactionStats};
pub use state::AccountState;
