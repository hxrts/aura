#![allow(clippy::disallowed_methods, clippy::disallowed_types)]
//! # Aura Rendezvous - Layer 5: Feature/Protocol Implementation
//!
//! This crate implements Secret-Branded Broadcasting (SBB) and rendezvous protocols
//! for privacy-preserving communication in the Aura threshold identity platform.
//!
//! ## Purpose
//!
//! Layer 5 feature crate providing privacy-preserving communication protocols for:
//! - Secret-Branded Broadcasting (SBB) for metadata-private message delivery
//! - Relationship-scoped secure channels between peers
//! - Capability-based relay node selection and routing
//! - Context-aware rendezvous coordination for peer discovery
//! - Anonymous messaging with full relationship isolation
//!
//! ## Architecture Constraints
//!
//! This crate depends on:
//! - **Layer 1** (aura-core): Core types, effects, errors
//! - **Layer 2** (aura-journal, aura-transport): Domain semantics and transport
//! - **Layer 3** (aura-effects): Effect handler implementations
//! - **Layer 4** (aura-protocol): Orchestration and guard chain
//! - **Layer 4** (aura-mpst): Session types for SBB coordination
//! - **Layer 5** (aura-relational): Relationship context management
//!
//! ## What Belongs Here
//!
//! - Complete SBB protocol implementations (flooding, coordination)
//! - Secure channel establishment and key rotation protocols
//! - Relay selection with capability-based filtering
//! - Context-aware rendezvous for peer discovery
//! - Encryption and key derivation for relationships
//! - Anonymous messaging envelope handling
//! - MPST protocol definitions for rendezvous ceremonies
//!
//! ## What Does NOT Belong Here
//!
//! - Effect handler implementations (belong in aura-effects)
//! - Handler composition or registry (belong in aura-composition)
//! - Low-level multi-party coordination (belong in aura-protocol)
//! - Transport layer implementations (belong in aura-transport)
//! - Relationship management (belong in aura-relational)
//!
//! ## Design Principles
//!
//! - Metadata privacy through SBB: routing information is cryptographically hidden
//! - All channel state is ephemeral; relationships persist in relational contexts
//! - Relay selection is capability-scoped: only authorized relays are visible
//! - Rendezvous protocols are transient: contexts dissolve after peer exchange
//! - Integration with guard chain ensures authorization before routing
//! - Messages are padded to uniform size for traffic analysis resistance
//!
//! ## Key Protocols
//!
//! - **Secret-Branded Broadcasting**: Metadata-private message flooding
//! - **Secure Channel**: End-to-end encryption with forward secrecy
//! - **Relay Selection**: Capability-based relay filtering and routing
//! - **Rendezvous**: Privacy-preserving peer discovery and contact exchange

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

// Re-export lightweight crypto stand-ins
pub use crypto::{BlindSignature, SecretBrand, UnlinkableCredential};

// Re-export capability types from journal
pub use aura_journal::CapabilityRef;

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
