#![allow(clippy::disallowed_methods, clippy::disallowed_types)]
//! # Aura Authenticate - Layer 5: Feature/Protocol Implementation
//!
//! **Purpose**: Device, threshold, and guardian authentication protocols.
//!
//! Complete end-to-end authentication protocols using stateless effect composition.
//! Provides authentication coordinators for device authentication, session establishment,
//! and guardian authentication for recovery operations.
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
//! - Device Authentication: Challenge-response with capability verification
//! - Session Establishment: Session ticket creation with time-limited capabilities
//! - Guardian Authentication: M-of-N guardian approval for recovery
//!
//! # Design Principles
//!
//! - Effect Composition: Stateless effect handlers for predictable execution
//! - Capability Verification: Effect-based capability checking and enforcement
//! - Journal Integration: CRDT state management through effect system
//! - Privacy Enforcement: Effect-level privacy controls and audit trails
//! - Composable: Reusable authentication building blocks

#![allow(missing_docs)]
#![forbid(unsafe_code)]

/// Device authentication coordinator
pub mod device_auth;

/// Authority authentication coordinator
pub mod authority_auth;

/// Session establishment coordinator
pub mod session_creation;

/// Guardian authentication coordinator for recovery operations
pub mod guardian_auth;

/// Guardian authentication via relational contexts (new model)
pub mod guardian_auth_relational;

/// Distributed Key Derivation (DKD) protocol implementation
pub mod dkd;

// Re-export core types from aura-core (Layer 1)
pub use aura_core::{AccountId, AuraError, AuraResult, Journal};

// Re-export verification types from aura-verify (Layer 2)
pub use aura_verify::session::{SessionScope, SessionTicket};
pub use aura_verify::{
    AuthenticationError, IdentityProof, KeyMaterial, Result as AuthenticationResult,
    VerifiedIdentity,
};

// Re-export Biscuit authorization types
pub use aura_protocol::guards::{BiscuitGuardEvaluator, GuardError, GuardResult};
pub use aura_wot::{AccountAuthority, BiscuitTokenManager, ResourceScope};

// Re-export DKD types
pub use dkd::{
    create_test_config, execute_simple_dkd, DkdConfig, DkdError, DkdProtocol, DkdResult,
    DkdSessionId, KeyDerivationContext, ParticipantContribution,
};
