//! Aura FROST Choreographies
//!
//! This crate provides choreographic protocols for FROST threshold signature
//! operations in the Aura threshold identity platform.
//!
//! # Architecture
//!
//! This crate implements FROST choreographies:
//! - `G_frost` - Main threshold signing choreography
//! - `G_dkg` - Distributed key generation choreography  
//! - `key_resharing` - Key redistribution protocols
//! - `signature_aggregation` - Multi-round signature coordination
//!
//! # Design Principles
//!
//! - Uses choreographic programming for distributed FROST coordination
//! - Integrates with aura-crypto for real cryptographic operations
//! - Provides clean separation to avoid namespace conflicts (E0428 errors)
//! - Supports M-of-N threshold configurations with Byzantine fault tolerance

#![warn(missing_docs)]
#![forbid(unsafe_code)]

/// Main FROST threshold signing choreography (G_frost)
pub mod threshold_signing;

/// Distributed key generation choreography (G_dkg)
pub mod distributed_keygen;

/// Key resharing and rotation protocols
pub mod key_resharing;

/// Signature aggregation and verification
pub mod signature_aggregation;

/// Errors for FROST operations
// errors module removed - use aura_core::AuraError directly

// Re-export core types
pub use aura_core::{AccountId, AuraError, AuraResult, Cap, DeviceId, Journal};

// Re-export crypto types
pub use aura_crypto::frost::{
    Nonce, NonceCommitment, PartialSignature, PublicKeyPackage, Share, SigningSession,
    ThresholdSignature, TreeSigningContext,
};

// Re-export core effect types  
pub use aura_core::effects::{NetworkEffects, CryptoEffects, TimeEffects, ConsoleEffects};

// Re-export FROST coordinators and choreographies
pub use distributed_keygen::DkgCoordinator;
pub use key_resharing::KeyResharingCoordinator;
pub use signature_aggregation::SignatureAggregationCoordinator;
pub use threshold_signing::FrostSigningCoordinator;

// Type aliases for this crate
pub type FrostResult<T> = Result<T, AuraError>;
pub type FrostError = AuraError;

// Error re-exports removed - use aura_core::AuraError directly
