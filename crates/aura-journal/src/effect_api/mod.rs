//! Layer 2: Journal Effect API - Intent & Capability Management
//!
//! Implements journal CRDT components for intent staging and capability-based authorization.
//! Separates authentication (tree membership via AttestedOp) from authorization (fine-grained capabilities).
//!
//! **Key Types**:
//! - **Intent**: Proposed tree mutations staged for batch processing
//! - **CapabilityRef**: Fine-grained, revocable authorization tokens with expiry/scope
//! - **IntentBatch**: Atomic group of intents for transactional tree updates
//!
//! **Design Principle** (per docs/104_authorization.md):
//! All security-critical mutations recorded as AttestedOp (threshold signatures) in fact journal.
//! Capabilities provide layered authorization via Biscuit tokens (aura-authorization/biscuit) evaluated at
//! message entry point (aura-protocol/guards/CapGuard).

pub mod capability;
pub mod intent;
pub mod journal_types;

// Re-export key types
pub use capability::{CapabilityId, CapabilityRef, ResourceRef};
pub use intent::{Intent, IntentBatch, IntentId, IntentStatus, Priority};
pub use journal_types::{JournalError, JournalStats};
