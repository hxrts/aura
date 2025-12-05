//! Unified Threshold Signing Types
//!
//! This module provides the context types for unified threshold signing across
//! all scenarios: multi-device, guardian recovery, and group operations.
//!
//! The core insight is that multi-device signing, guardian recovery, and group
//! operations are mathematically identical - they're all threshold signature schemes.
//! The only differences are:
//! - **Who** the participants are (devices, guardians, group members)
//! - **Why** they're signing (personal op, recovery, group decision)
//!
//! # Architecture
//!
//! ```text
//! SigningContext
//! ├── authority: AuthorityId       // Whose keys are signing
//! ├── operation: SignableOperation // What's being signed
//! └── approval_context: ApprovalContext // Why (for audit/display)
//! ```
//!
//! The same `ThresholdSigningService` and choreographies handle all scenarios
//! by parameterizing on `SigningContext`.

mod context;
mod participant;
mod signature;

pub use context::{ApprovalContext, GroupAction, SignableOperation, SigningContext};
pub use participant::{ParticipantEndpoint, ParticipantIdentity, SigningParticipant};
pub use signature::ThresholdSignature;

// Re-export ThresholdConfig from crypto::frost for convenience
pub use crate::crypto::frost::ThresholdConfig;
