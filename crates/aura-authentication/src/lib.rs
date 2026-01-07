#![allow(clippy::disallowed_methods, clippy::disallowed_types)]
//! # Aura Authenticate - Layer 5: Feature/Protocol Implementation
//!
//! **Purpose**: Authority, threshold, and guardian authentication protocols.
//!
//! Complete end-to-end authentication protocols using the guard chain pattern.
//! Provides `AuthService` for authentication operations with pure guard evaluation
//! and explicit effect execution.
//!
//! # Architecture Constraints
//!
//! **Layer 5 depends on aura-core, aura-effects, aura-composition, aura-protocol, aura-mpst, and domain crates**.
//! - MUST build on orchestration layer (aura-protocol)
//! - MUST compose effects from aura-effects and aura-composition
//! - MUST implement end-to-end protocol logic
//! - MUST NOT implement effect handlers (that's aura-effects)
//! - MUST NOT implement orchestration primitives (that's aura-protocol)
//! - MUST NOT do UI or CLI concerns (that's Layer 7)
//!
//! # Core Protocols
//!
//! - Challenge-Response Authentication: Request → Challenge → Proof → Session
//! - Session Management: Time-limited capabilities with scope restrictions
//! - Guardian Authentication: M-of-N guardian approval for recovery operations
//! - Distributed Key Derivation: Multi-party key generation without revealing shares
//!
//! # Design Principles
//!
//! - **Guard Chain Pattern**: Pure evaluation over `GuardSnapshot` → `GuardOutcome` → `EffectCommand` execution
//! - **Fact-Based State**: All state changes recorded as immutable `AuthFact` records
//! - **View Derivation**: State derived from facts via `AuthViewReducer`
//! - **Capability Verification**: Guard-based capability checking before operations
//! - **Authority-Centric**: Uses `AuthorityId` as the primary identity type
//!
//! # Module Organization
//!
//! - [`guards`]: Pure guard types (`GuardSnapshot`, `GuardOutcome`, `EffectCommand`, `RecoveryContext`)
//! - [`facts`]: Domain fact types (`AuthFact`, `AuthFactReducer`, `AuthFactDelta`)
//! - [`service`]: Main `AuthService` with guard chain integration
//! - [`view`]: View types (`AuthView`, `AuthViewReducer`) for deriving state from facts
//! - [`guardian_auth_relational`]: Relational context-based guardian authentication
//! - [`dkd`]: Distributed Key Derivation protocol
//!
//! See `docs/100_authority_and_identity.md` for the authority model documentation.

#![allow(missing_docs)]
#![forbid(unsafe_code)]

// =============================================================================
// Core Modules (New Architecture)
// =============================================================================

/// Guard types for authentication operations
///
/// Provides `GuardSnapshot`, `GuardOutcome`, `EffectCommand`, and related types
/// for guard chain integration following the pattern from `aura-invitation`.
pub mod guards;

/// Domain fact types for authentication state changes
pub mod facts;

/// Authentication service coordinator
///
/// Main service for authentication operations with guard chain integration.
/// All operations return `GuardOutcome` for the caller to execute.
pub mod service;

/// View delta and reducer for authentication facts
///
/// Provides `AuthView`, `AuthViewReducer`, and related view types
/// for deriving authentication state from the fact log.
pub mod view;

/// Guardian authentication via relational contexts
///
/// Authority-centric guardian authentication using `RelationalContext`.
pub mod guardian_auth_relational;

/// Distributed Key Derivation (DKD) protocol implementation
pub mod dkd;

/// Operation category map (A/B/C) for protocol gating and review.
///
/// Note: Categories should be reviewed against `docs/117_operation_categories.md`.
pub const OPERATION_CATEGORIES: &[(&str, &str)] = &[
    ("auth:challenge", "A"),
    ("auth:proof", "A"),
    ("auth:session-issue", "A"),
    ("auth:session-revoke", "A"),
    ("auth:guardian-approval", "C"),
    ("auth:recovery-complete", "C"),
];

/// Lookup the operation category (A/B/C) for a given operation.
pub fn operation_category(operation: &str) -> Option<&'static str> {
    OPERATION_CATEGORIES
        .iter()
        .find(|(op, _)| *op == operation)
        .map(|(_, category)| *category)
}

// Re-export core types from aura-core (Layer 1)
pub use aura_core::{AccountId, AuraError, AuraResult, Journal};

// Re-export verification types from aura-signature (Layer 2)
pub use aura_signature::session::{SessionScope, SessionTicket};
pub use aura_signature::{
    AuthenticationError, IdentityProof, KeyMaterial, Result as AuthenticationResult,
    VerifiedIdentity,
};

// Re-export Biscuit authorization types
pub use aura_authorization::{BiscuitTokenManager, ResourceScope, TokenAuthority};
pub use aura_guards::{BiscuitGuardEvaluator, GuardError, GuardResult};

// Re-export DKD types
pub use dkd::{
    create_test_config, execute_simple_dkd, DkdConfig, DkdError, DkdProtocol, DkdResult,
    DkdSessionId, KeyDerivationContext, ParticipantContribution,
};

// Re-export guard types
pub use guards::{
    check_capability, check_flow_budget, costs, evaluate_request, EffectCommand, GuardDecision,
    GuardOutcome, GuardRequest, GuardSnapshot, RecoveryContext, RecoveryOperationType,
};

// Re-export fact types
pub use facts::{AuthFact, AuthFactDelta, AuthFactReducer, AUTH_FACT_TYPE_ID};

// Re-export service types
pub use service::{
    AuthService, AuthServiceConfig, ChallengeResult, GuardianApprovalResult, SessionResult,
};

// Re-export view types
pub use view::{
    AuthView, AuthViewReducer, ChallengeInfo, FailureRecord, RecoveryInfo, SessionInfo,
};

// =============================================================================
// Generated Runner Re-exports for execute_as Pattern
// =============================================================================

/// Re-exports for DkdChoreography runners
pub mod dkd_runners {
    pub use crate::dkd::rumpsteak_session_types_dkd_protocol::dkd_protocol::runners::{
        execute_as, run_initiator, run_participant, InitiatorOutput, ParticipantOutput,
    };
    pub use crate::dkd::rumpsteak_session_types_dkd_protocol::dkd_protocol::DkdChoreographyRole;
}

/// Re-exports for GuardianAuthRelational choreography runners
pub mod guardian_auth_runners {
    pub use crate::guardian_auth_relational::rumpsteak_session_types_guardian_auth_relational::guardian_auth_relational::GuardianAuthRelationalRole;
    pub use crate::guardian_auth_relational::rumpsteak_session_types_guardian_auth_relational::guardian_auth_relational::runners::{
        execute_as, run_account, run_coordinator, run_guardian,
        AccountOutput, CoordinatorOutput, GuardianOutput,
    };
}
