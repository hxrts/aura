//! # Aura Recovery - Layer 5: Feature/Protocol Implementation
//!
//! This crate implements guardian-based recovery protocols for threshold identity
//! management in the Aura platform.
//!
//! ## Purpose
//!
//! Layer 5 feature crate providing end-to-end protocol implementations for:
//! - Guardian setup and initial relationship establishment
//! - Guardian membership changes (adding/removing guardians)
//! - Emergency key recovery with guardian approval
//! - Recovery initiation and multi-party recovery coordination
//!
//! ## Architecture Constraints
//!
//! This crate depends on:
//! - **Layer 1** (aura-core): Core types, effects, errors
//! - **Layer 2** (aura-journal, aura-verify, aura-transport): Domain semantics
//! - **Layer 3** (aura-effects): Effect handler implementations
//! - **Layer 4** (aura-protocol): Orchestration and guard chain
//! - **Layer 5** (aura-authenticate): Authentication coordination
//! - **Layer 5** (aura-relational): Relational context management for recovery
//!
//! ## What Belongs Here
//!
//! - Complete guardian recovery protocol implementations
//! - Guardian setup choreographies for threshold establishment
//! - Guardian membership change coordination (add/remove)
//! - Emergency key recovery protocols with multi-party approval
//! - Recovery context management and state coordination
//! - MPST protocol definitions for recovery ceremonies
//!
//! ## What Does NOT Belong Here
//!
//! - Effect handler implementations (belong in aura-effects)
//! - Handler composition or registry (belong in aura-composition)
//! - Low-level multi-party coordination (belong in aura-protocol)
//! - Guardian relationship definitions (belong in aura-relational)
//! - Cryptographic threshold signing (via core FROST primitives; aura-frost removed)
//!
//! ## Design Principles
//!
//! - Recovery ceremonies are structured, multi-step protocols
//! - All protocols respect threshold requirements (k-of-n approval)
//! - Integration with guard chain ensures authorization before recovery
//! - Recovery state lives in relational contexts, not local authority state
//! - Transactional semantics: recovery either completes or cleanly aborts
//!
//! ## Key Protocols
//!
//! - **Guardian Setup**: Initial k-of-n guardian threshold establishment
//! - **Membership Change**: Adding or removing guardians from recovery set
//! - **Key Recovery**: Emergency key recovery initiated by device owner
//! - **Recovery Coordination**: Multi-party agreement on recovery terms

#![allow(missing_docs)]
#![forbid(unsafe_code)]

/// Recovery domain facts for journal integration
pub mod facts;

/// Recovery view deltas for reactive UI updates
pub mod view;

/// Common utilities for recovery operations (DRY infrastructure)
pub mod utils;

/// Base coordinator infrastructure for all recovery operations
pub mod coordinator;

/// Guardian setup choreography for initial relationship establishment
pub mod guardian_setup;

/// Guardian key recovery approvals
pub mod guardian_key_recovery;

/// Guardian membership change choreography for adding/removing guardians
pub mod guardian_membership;

/// Recovery protocol using relational contexts
pub mod recovery_protocol;

/// Shared types for guardian operations
pub mod types;

// Core error types
pub use aura_core::{AuraError, AuraResult};

/// Recovery-specific error type
pub type RecoveryError = AuraError;

/// Recovery-specific result type
pub type RecoveryResult<T> = AuraResult<T>;

// Re-export essential types
pub use types::{GuardianProfile, GuardianSet, RecoveryRequest, RecoveryResponse};

// Re-export auth types
pub use aura_authenticate::guardian_auth::{
    GuardianAuthCoordinator, GuardianAuthRequest, GuardianAuthResponse, RecoveryContext,
    RecoveryOperationType,
};

// Re-export choreography coordinators
pub use guardian_membership::GuardianMembershipCoordinator;
pub use guardian_setup::GuardianSetupCoordinator;

// Re-export new recovery protocol
pub use recovery_protocol::{RecoveryProtocol, RecoveryProtocolHandler};

// Re-export Biscuit types for convenience
pub use aura_core::scope::ResourceScope;
pub use aura_protocol::guards::BiscuitGuardEvaluator;
pub use aura_wot::BiscuitTokenManager;

// Re-export membership change types
pub use guardian_membership::{MembershipChange, MembershipChangeRequest};

// Re-export facts for registry integration
pub use facts::{MembershipChangeType, RecoveryFact, RecoveryFactReducer, RECOVERY_FACT_TYPE_ID};

// Re-export view deltas for UI integration
pub use view::{RecoveryDelta, RecoveryViewReducer};
