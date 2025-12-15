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
//! - **Layer 2** (aura-journal, aura-verify, aura-wot, aura-macros, aura-mpst): Domain semantics and choreography
//! - **Layer 3** (aura-effects, aura-composition): Effect handler implementations
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

/// Effect composition for recovery operations
pub mod effects;

/// Fact-derived state for recovery operations
pub mod state;

/// Common utilities for recovery operations (DRY infrastructure)
pub mod utils;

/// Base coordinator infrastructure for all recovery operations
pub mod coordinator;

/// Guardian setup choreography for initial relationship establishment
pub mod guardian_setup;

/// Consensus-based guardian ceremony with linear protocol guarantees
pub mod guardian_ceremony;

/// Guardian key recovery approvals
pub mod guardian_key_recovery;

/// Guardian membership change choreography for adding/removing guardians
pub mod guardian_membership;

/// Recovery protocol using relational contexts
pub mod recovery_protocol;

/// Consensus-based recovery approval ceremony
pub mod recovery_ceremony;

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

// Re-export auth types (from new guard-based architecture)
pub use aura_authenticate::{RecoveryContext, RecoveryOperationType};

// Re-export choreography coordinators
pub use guardian_membership::GuardianMembershipCoordinator;
pub use guardian_setup::GuardianSetupCoordinator;

// Re-export consensus-based ceremony
pub use guardian_ceremony::{
    CeremonyId, CeremonyResponse, CeremonyState, CeremonyStatus, GuardianCeremonyExecutor,
    GuardianCeremonyManager, GuardianRotationOp, GuardianState,
};

// Re-export new recovery protocol
pub use recovery_protocol::{RecoveryOutcome, RecoveryProtocol, RecoveryProtocolHandler};

// Re-export membership change types
pub use guardian_membership::{MembershipChange, MembershipChangeRequest};

// Re-export facts for registry integration
pub use facts::{
    MembershipChangeType, RecoveryFact, RecoveryFactEmitter, RecoveryFactReducer,
    RECOVERY_FACT_TYPE_ID,
};

// Re-export view deltas for UI integration
pub use view::{RecoveryDelta, RecoveryViewReducer};

// Re-export composed effect traits for minimal effect bounds
pub use effects::{RecoveryEffects, RecoveryNetworkEffects};

// Re-export state types for fact-derived state
pub use state::{
    MembershipProposalState, ProposalStatus, RecoveryOperationState, RecoveryState, RecoveryStatus,
    SetupState, SetupStatus,
};

// Re-export consensus-based recovery ceremony
pub use recovery_ceremony::{
    CeremonyRecoveryOperation, CeremonyRecoveryRequest, RecoveryApproval, RecoveryCeremonyConfig,
    RecoveryCeremonyEffects, RecoveryCeremonyExecutor, RecoveryCeremonyFact, RecoveryCeremonyId,
    RecoveryCeremonyState, RecoveryCeremonyStatus,
};
