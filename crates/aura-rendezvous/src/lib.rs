//! Aura Rendezvous Layer
//!
//! This crate provides Secret-Branded Broadcasting (SBB) and rendezvous protocols
//! for privacy-preserving communication in the Aura threshold identity platform.
//!
//! # Architecture
//!
//! This crate implements rendezvous application layer:
//! - `sbb/` - Secret-Branded Broadcasting protocols
//! - `discovery/` - Privacy-preserving peer discovery
//! - `messaging/` - Anonymous messaging with relationship isolation
//! - `relay/` - Relay coordination and capability-based routing
//!
//! # Design Principles
//!
//! - Uses Secret-Branded Broadcasting for metadata privacy
//! - Implements relationship-scoped communication contexts
//! - Provides capability-based relay authorization
//! - Integrates with choreographic programming for coordination

#![allow(missing_docs)]
#![forbid(unsafe_code)]

/// Secret-Branded Broadcasting protocols
pub mod sbb;

/// Privacy-preserving peer discovery
pub mod discovery;

/// Anonymous messaging with relationship isolation  
pub mod messaging;

/// Relay coordination and capability-based routing
pub mod relay;

/// Relationship key derivation for SBB encryption
pub mod relationship_keys;

/// Envelope encryption with HPKE and padding
pub mod envelope_encryption;

/// Capability-aware SBB flooding with WoT integration
pub mod capability_aware_sbb;

/// Complete integrated SBB system
pub mod integrated_sbb;

/// End-to-end integration tests for SBB system
#[cfg(test)]
pub mod integration_tests;

/// Rendezvous-specific errors
pub mod error;

/// Connection priority management for NAT traversal
pub mod connection_manager;

/// Relay selection heuristics with guardian preference
pub mod relay_selection;

mod crypto;

// Re-export core types
pub use aura_core::{AccountId, AuraError, AuraResult, Cap, DeviceId, RelationshipId};

// Re-export crypto placeholder types
pub use crypto::{BlindSignature, SecretBrand, UnlinkableCredential};

// Re-export WoT types for capabilities
pub use aura_wot::{Capability, RelayPermission, TrustLevel};

// Re-export protocol effect system
pub use aura_protocol::AuraEffectSystem;

// Re-export main APIs
pub use capability_aware_sbb::{
    CapabilityAwareSbbCoordinator, SbbFlowBudget, SbbForwardingPolicy, SbbRelationship,
    TrustStatistics,
};
pub use connection_manager::{
    ConnectionConfig, ConnectionManager, ConnectionMethod, ConnectionResult,
};
pub use discovery::{DiscoveryQuery, DiscoveryService, RendezvousPoint};
pub use envelope_encryption::{EncryptedEnvelope, EnvelopeEncryption, PaddingStrategy};
pub use integrated_sbb::{
    IntegratedSbbSystem, SbbConfig, SbbDiscoveryRequest, SbbDiscoveryResult, SbbSystemBuilder,
};
pub use messaging::{
    MockTransportSender, NetworkTransportSender, SbbMessageType, SbbTransportBridge,
    TransportMethod, TransportOfferPayload, TransportSender,
};
pub use relationship_keys::{
    derive_test_root_key, RelationshipContext, RelationshipKey, RelationshipKeyManager,
};
pub use relay::{
    RelayCapabilities, RelayCoordinator, RelayNode, RelayStream, StreamFlags, StreamState,
};
pub use relay_selection::{
    RelayCandidate, RelaySelectionConfig, RelaySelectionResult, RelaySelector, RelayType,
};
pub use sbb::{
    EnvelopeId, FloodResult, RendezvousEnvelope, SbbEnvelope, SbbFlooding, SbbFloodingCoordinator,
};
