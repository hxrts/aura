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
mod lifecycle;
mod policy;
mod participant;
mod signature;
mod types;

pub use context::{ApprovalContext, GroupAction, SignableOperation, SigningContext};
pub use lifecycle::{
    ConsensusLifecycle, CoordinatorLifecycle, ProvisionalLifecycle, RotationLifecycle,
    ThresholdLifecycle,
};
pub use policy::{policy_for, CeremonyFlow, CeremonyLifecyclePolicy, KeyGenerationPolicy};
pub use participant::{ParticipantEndpoint, ParticipantIdentity, SigningParticipant};
pub use signature::ThresholdSignature;
pub use types::{AgreementMode, ConvergenceCert, ReversionFact, RotateFact};

// Re-export ThresholdConfig and ThresholdState from crypto::frost for convenience
pub use crate::crypto::frost::{ThresholdConfig, ThresholdState};
