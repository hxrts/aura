//! Aura Rendezvous Layer
//!
//! This crate provides Secret-Branded Broadcasting (SBB) and rendezvous protocols
//! for privacy-preserving communication in the Aura threshold identity platform.
//!
//! # Architecture
//!
//! This crate implements rendezvous application layer:
//! - `sbb/` - Secret-Branded Broadcasting protocols
//! - `crypto/` - Encryption and key management
//! - `relay/` - Relay coordination and capability-based routing
//! - `messaging/` - Anonymous messaging with relationship isolation
//! - `context/` - Context-aware rendezvous system
//! - `channel/` - Secure channel establishment
//! - `integration/` - Complete integrated SBB system
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

/// Cryptographic primitives and key management
pub mod crypto;

/// Relay coordination and capability-based routing
pub mod relay;

/// Anonymous messaging with relationship isolation
pub mod messaging;

/// Context-aware rendezvous system
pub mod context;

/// Secure channel establishment protocols
pub mod channel;

/// Complete integrated SBB system
pub mod integration;

/// Privacy-preserving peer discovery (legacy, to be refactored)
pub mod discovery;

/// Rendezvous-specific errors
pub mod error;


// Keep legacy top-level modules for backward compatibility during transition
// These will be removed once all imports are updated
#[doc(hidden)]
pub use crate::channel::secure as secure_channel;
#[doc(hidden)]
pub use crate::context::rendezvous as context_rendezvous;
#[doc(hidden)]
pub use crate::crypto::encryption as envelope_encryption;
#[doc(hidden)]
pub use crate::crypto::keys as relationship_keys;
#[doc(hidden)]
pub use crate::integration::capability_aware as capability_aware_sbb;
#[doc(hidden)]
pub use crate::integration::connection as connection_manager;
#[doc(hidden)]
pub use crate::integration::sbb_system as integrated_sbb;
#[doc(hidden)]
pub use crate::relay::selection as relay_selection;

// Re-export core types from aura-core
pub use aura_core::{AccountId, AuraError, AuraResult, Cap, RelationshipId, TrustLevel};

// Re-export crypto placeholder types
pub use crypto::{BlindSignature, SecretBrand, UnlinkableCredential};

// Re-export capability types from journal
pub use aura_journal::Capability;

// Re-export SBB types
pub use sbb::{
    EnvelopeId, FloodResult, RendezvousEnvelope, SbbEnvelope, SbbFlooding, SbbFloodingCoordinator,
    SBB_MESSAGE_SIZE,
};

// Re-export crypto types
pub use crypto::{
    derive_test_root_key, EncryptedEnvelope, EnvelopeEncryption, PaddingStrategy,
    RelationshipContext, RelationshipKey, RelationshipKeyManager,
};

// Re-export relay types
pub use relay::{
    RelayCandidate, RelayCapabilities, RelayNode, RelaySelectionConfig, RelaySelectionResult,
    RelaySelector, RelayStream, RelayType, StreamFlags, StreamState,
};

// Note: RelayCoordinator and related types are still in the legacy relay.rs file
// and need to be properly extracted during full refactoring

// Re-export messaging types
pub use messaging::{
    MockTransportSender, NetworkConfig, NetworkTransport, NetworkTransportSender, SbbMessageType,
    SbbTransportBridge, TransportMethod, TransportOfferPayload, TransportSender,
};

// Re-export channel types
pub use channel::{
    ChannelConfig, ChannelLifecycleState, ChannelState, HandshakeComplete, HandshakeInit,
    HandshakeResponse, HandshakeResult, KeyRotationRequest, SecureChannelCoordinator,
};

// Re-export context rendezvous types
pub use context::{
    ContextEnvelope, ContextRendezvousCoordinator, ContextRendezvousDescriptor,
    ContextTransportBridge, ContextTransportOffer, RendezvousReceipt,
};

// Re-export integration types
pub use integration::{
    CapabilityAwareSbbCoordinator, ConnectionConfig, ConnectionManager, ConnectionMethod,
    ConnectionResult, IntegratedSbbSystem, PunchResult, PunchSession, QuicConfig, SbbConfig,
    SbbDiscoveryRequest, SbbDiscoveryResult, SbbFlowBudget, SbbForwardingPolicy, SbbRelationship,
    SbbSystemBuilder, StunClient, StunResult, TrustStatistics,
};

// Re-export discovery types
pub use discovery::{DiscoveryQuery, DiscoveryService, RendezvousPoint};
