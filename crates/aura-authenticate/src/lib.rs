//! Aura Authentication Choreographies
//!
//! This crate provides choreographic protocols for distributed authentication
//! across devices in the Aura threshold identity platform.
//!
//! # Architecture
//!
//! This crate implements authentication choreographies:
//! - `G_auth` - Main device authentication choreography
//! - `session_establishment` - Distributed session ticket creation
//! - `guardian_auth` - Multi-guardian authentication for recovery
//!
//! # Design Principles
//!
//! - Uses choreographic programming for distributed auth coordination
//! - Integrates with aura-verify for identity verification
//! - Provides clean separation to avoid namespace conflicts (E0428 errors)
//! - Works with threshold signatures and guardian approval workflows

#![warn(missing_docs)]
#![forbid(unsafe_code)]

/// Main authentication choreography (G_auth)
pub mod device_auth;

/// Session establishment protocols
pub mod session_establishment;

/// Guardian authentication for recovery operations
pub mod guardian_auth;

/// Errors for authentication operations
// errors module removed - use aura_core::AuraError directly

// Re-export core types
pub use aura_core::{AccountId, AuraError, AuraResult, Cap, DeviceId, Journal};

// Re-export verification types
pub use aura_verify::session::{SessionScope, SessionTicket};
pub use aura_verify::{
    AuthenticationError, IdentityProof, KeyMaterial, Result as AuthenticationResult,
    VerifiedIdentity,
};

// Re-export MPST types
pub use aura_mpst::{
    AuraRuntime, CapabilityGuard, ExecutionContext, JournalAnnotation, MpstError, MpstResult,
};

// Error re-exports removed - use aura_core::AuraError directly
