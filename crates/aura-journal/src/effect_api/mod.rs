//! Effect API Module
//!
//! Implements the journal effect_api CRDT that stores the authoritative history of tree operations.
//! This module provides:
//!
//! - **AttestedOp**: Signed operations that mutate the commitment tree
//! - **CapabilityRef**: Authorization tokens with expiry and scope
//! - **Intent**: Proposed tree mutations staged in the intent pool
//!
//! ## Architecture
//!
//! The effect_api separates authentication (tree membership) from authorization (capabilities).
//! All security-critical tree mutations are recorded as signed TreeOp entries with threshold
//! attestation, while capabilities provide fine-grained, revocable authorization tokens.

pub mod capability;
pub mod intent;
pub mod journal_types;

// Re-export key types
pub use capability::{CapabilityId, CapabilityRef, ResourceRef};
pub use intent::{Intent, IntentBatch, IntentId, IntentStatus, Priority};
pub use journal_types::{JournalError, JournalStats};
