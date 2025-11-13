//! Aura Authentication Choreographies
//!
//! **Layer 5: Feature/Protocol Implementation**
//!
//! Complete end-to-end authentication protocols using stateless effect composition.
//! Provides three main authentication coordinators: device authentication,
//! session establishment, and guardian authentication for recovery operations.
//!
//! # Architecture
//!
//! Sits in Layer 5 of Aura's 8-layer architecture. Depends on `aura-core`, `aura-verify`,
//! `aura-effects`, and `aura-protocol`. Used by runtime layers (`aura-agent`) and UI layers.
//!
//! # Coordinators
//!
//! - **Device Authentication**: Challenge-response protocol with capability verification
//!   and journal state management for device authentication
//! - **Session Establishment**: Distributed session ticket creation with time-limited
//!   capabilities and defined scopes (DKD, storage, etc.)
//! - **Guardian Authentication**: M-of-N guardian approval coordinator for sensitive
//!   recovery operations
//!
//! # Design Principles
//!
//! - **Effect Composition**: Stateless effect handlers for predictable execution
//! - **Capability Verification**: Effect-based capability checking and enforcement
//! - **Journal Integration**: CRDT state management through effect system
//! - **Privacy Enforcement**: Effect-level privacy controls and audit trails
//! - **Composable**: Reusable authentication building blocks for applications

#![allow(missing_docs)]
#![forbid(unsafe_code)]

/// Device authentication coordinator
pub mod device_auth;

/// Session establishment coordinator
pub mod session_establishment;

/// Guardian authentication coordinator for recovery operations
pub mod guardian_auth;

// Re-export core types from aura-core (Layer 1)
pub use aura_core::{AccountId, AuraError, AuraResult, Cap, DeviceId, Journal};

// Re-export verification types from aura-verify (Layer 2)
pub use aura_verify::session::{SessionScope, SessionTicket};
pub use aura_verify::{
    AuthenticationError, IdentityProof, KeyMaterial, Result as AuthenticationResult,
    VerifiedIdentity,
};

// Re-export effect system types
pub use aura_protocol::AuraEffectSystem;
