//! Aura FROST Choreographies
//!
//! This crate provides choreographic protocols for FROST threshold signature
//! operations in the Aura threshold identity platform.
//!
//! # Architecture
//!
//! This crate implements FROST protocols using the stateless effect system:
//! - `threshold_signing` - FROST threshold signing service
//! - `distributed_keygen` - Distributed key generation service
//! - `key_resharing` - Key redistribution services
//! - `signature_aggregation` - Multi-round signature coordination
//!
//! # Design Principles
//!
//! - Uses stateless effect composition for distributed FROST coordination
//! - Integrates with aura-core for real cryptographic operations
//! - Effect-based architecture for predictable execution
//! - Supports M-of-N threshold configurations with Byzantine fault tolerance

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
pub use aura_core::{AccountId, AuraError, AuraResult, Cap, DeviceId, Journal};

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
