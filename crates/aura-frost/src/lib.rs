//! # Aura FROST - Layer 5: Feature/Protocol Implementation
//!
//! **Purpose**: FROST threshold signatures and key resharing operations.
//!
//! Choreographic protocols for FROST threshold signature operations in the Aura
//! threshold identity platform.
//!
//! # Architecture Constraints
//!
//! **Layer 5 depends on aura-core, aura-effects, aura-mpst, and domain crates**.
//! - MUST compose effects from aura-effects
//! - MUST implement end-to-end FROST ceremony logic
//! - MUST NOT implement cryptographic primitives directly (use CryptoEffects)
//! - MUST NOT implement orchestration primitives (that's Layer 4 aura-protocol)
//! - MUST NOT depend on runtime or UI layers (Layer 6+)
//! - MUST NOT do UI or CLI concerns (that's Layer 7)
//!
//! # Core Protocols
//!
//! - Threshold Signing: FROST threshold signature ceremonies
//! - Distributed Keygen: Distributed key generation service
//! - Key Resharing: Key redistribution with threshold changes
//! - Signature Aggregation: Multi-round signature coordination
//!
//! # Design Principles
//!
//! - Stateless effect composition for distributed coordination
//! - Byzantine fault tolerance for M-of-N configurations
//! - Effect-based architecture for predictable execution
//! - Composable threshold operations

#![allow(missing_docs)]
#![allow(
    unused_variables,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::type_complexity,
    clippy::while_let_loop,
    dead_code,
    clippy::redundant_closure,
    clippy::get_first,
    unused_must_use
)]

/// FROST threshold signing choreography using rumpsteak-aura (G_frost)
pub mod threshold_signing;

/// Distributed key generation choreography (G_dkg)
pub mod distributed_keygen;

/// Key resharing and rotation protocols
pub mod key_resharing;

/// Signature aggregation and verification
pub mod signature_aggregation;

// Re-export core types
pub use aura_core::{AccountId, AuraError, AuraResult, Cap, Journal};

// Re-export crypto types
pub use aura_core::frost::{
    Nonce, NonceCommitment, PartialSignature, PublicKeyPackage, Share, SigningSession,
    ThresholdSignature, TreeSigningContext,
};

// Re-export FROST types and utilities
pub use signature_aggregation::{perform_frost_aggregation, validate_aggregation_config};
pub use threshold_signing::{FrostCrypto, SigningPhase, ThresholdSigningConfig};

// Re-export message types for choreographies
pub use distributed_keygen::{
    DkgFailure, DkgRequest, DkgResponse, DkgSuccess, ShareCommitment, ShareRevelation,
    VerificationResult as DkgVerificationResult,
};
pub use key_resharing::{
    ResharingRequest, ResharingResponse, SharePackage,
    VerificationResult as ResharingVerificationResult,
};
pub use signature_aggregation::{
    AggregationRequest, AggregationResponse, PartialSignatureSubmission,
};

// Type aliases for this crate
/// Result type for FROST operations
pub type FrostResult<T> = Result<T, AuraError>;
/// Error type for FROST operations
pub type FrostError = AuraError;
