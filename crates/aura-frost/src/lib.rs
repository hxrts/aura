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
//! - Integrates with aura-crypto for real cryptographic operations
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
pub use aura_crypto::frost::{
    Nonce, NonceCommitment, PartialSignature, PublicKeyPackage, Share, SigningSession,
    ThresholdSignature, TreeSigningContext,
};

// Re-export protocol effect system
pub use aura_protocol::AuraEffectSystem;

// Re-export FROST coordinators and choreographies
pub use distributed_keygen::{get_dkg_choreography, DkgCoordinator};
pub use key_resharing::{get_resharing_choreography, KeyResharingCoordinator};
pub use signature_aggregation::{get_aggregation_choreography, SignatureAggregationCoordinator};
pub use threshold_signing::{
    get_frost_choreography, FrostCoordinator, FrostSigner, ThresholdSigningConfig,
};

// Type aliases for this crate
/// Result type for FROST operations
pub type FrostResult<T> = Result<T, AuraError>;
/// Error type for FROST operations
pub type FrostError = AuraError;
