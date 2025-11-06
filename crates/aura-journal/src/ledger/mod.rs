//! Ledger Module
//!
//! Implements the journal ledger CRDT that stores the authoritative history of tree operations.
//! This module provides:
//!
//! - **TreeOp**: Signed operations that mutate the ratchet tree
//! - **CapabilityRef**: Authorization tokens with expiry and scope
//! - **Intent**: Proposed tree mutations staged in the intent pool
//! - **JournalMap**: CRDT ledger combining ops and intents
//!
//! ## Architecture
//!
//! The ledger separates authentication (tree membership) from authorization (capabilities).
//! All security-critical tree mutations are recorded as signed TreeOp entries with threshold
//! attestation, while capabilities provide fine-grained, revocable authorization tokens.

pub mod capability;
pub mod crdt;
pub mod intent;
pub mod journal_types;
pub mod tree_op;

// Re-export key types
pub use capability::{CapabilityId, CapabilityRef, ResourceRef};
pub use crdt::JournalMap;
pub use intent::{Intent, IntentBatch, IntentId, IntentStatus, Priority};
pub use journal_types::{JournalError, JournalStats};
pub use tree_op::{ThresholdSignature, TreeOp, TreeOpRecord};
