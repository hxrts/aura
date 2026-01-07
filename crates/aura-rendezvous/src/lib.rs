#![allow(clippy::disallowed_methods, clippy::disallowed_types)]
//! # Aura Rendezvous - Layer 5: Feature/Protocol Implementation
//!
//! This crate implements privacy-preserving peer discovery and secure channel
//! establishment for the Aura threshold identity platform.
//!
//! ## Purpose
//!
//! Layer 5 feature crate providing:
//! - Fact-based transport descriptors propagated via journal sync
//! - MPST choreographies for rendezvous and relay protocols
//! - Guard chain integration for capability-scoped authorization
//! - Secure channel establishment with epoch-based key rotation
//!
//! ## Architecture
//!
//! This crate depends on:
//! - **Layer 1** (aura-core): Core types, effects, errors
//! - **Layer 2** (aura-journal): Domain facts and reduction
//! - **Layer 4** (aura-mpst): Session types for choreographies
//!
//! ## Modules
//!
//! - [`facts`]: Domain fact types (RendezvousFact, RendezvousDescriptor)
//! - [`protocol`]: MPST choreography definitions
//! - [`service`]: RendezvousService coordinator with guard integration
//! - [`descriptor`]: Transport selection and probing
//! - [`new_channel`]: Secure channel with epoch rotation
//!
//! ## Design Principles
//!
//! - Fact-based: All rendezvous state stored as journal facts
//! - Guard-aware: Authorization checked before any operation
//! - Epoch-based: Channels support key rotation for forward secrecy
//! - Transport-agnostic: Supports direct QUIC and relayed connections

#![allow(missing_docs)]
#![forbid(unsafe_code)]

// =============================================================================
// MODULES
// =============================================================================

/// Domain fact types for rendezvous (stored in journal, propagated via sync)
pub mod facts;

/// MPST choreography definitions for rendezvous protocols
pub mod protocol;

/// RendezvousService - main coordinator for peer discovery and channel establishment
pub mod service;

/// Transport descriptor types and selection logic
pub mod descriptor;

/// SecureChannel wrapper with epoch-based key rotation
pub mod new_channel;

/// Flood propagation for rendezvous packet flooding through social topology
pub mod flood;

/// LAN discovery packet formats and configuration (pure types).
///
/// Runtime UDP sockets and tasks live in the agent runtime (Layer 6).
pub mod lan_discovery;

/// Operation category map (A/B/C) for protocol gating and review.
pub const OPERATION_CATEGORIES: &[(&str, &str)] = &[
    ("rendezvous:publish-descriptor", "A"),
    ("rendezvous:refresh-descriptor", "A"),
    ("rendezvous:establish-channel", "A"),
    ("rendezvous:relay-request", "A"),
];

/// Lookup the operation category (A/B/C) for a given operation.
pub fn operation_category(operation: &str) -> Option<&'static str> {
    OPERATION_CATEGORIES
        .iter()
        .find(|(op, _)| *op == operation)
        .map(|(_, category)| *category)
}

// =============================================================================
// RE-EXPORTS
// =============================================================================

// Re-export core types from aura-core
pub use aura_core::{AccountId, AuraError, AuraResult, Cap, RelationshipId, TrustLevel};

// Re-export capability types from journal
pub use aura_journal::CapabilityRef;

// Re-export fact types (for journal integration)
pub use facts::{
    RendezvousDescriptor, RendezvousFact, RendezvousFactReducer, TransportAddress,
    TransportAddressError, TransportHint, RENDEZVOUS_FACT_TYPE_ID,
};

// Re-export protocol types
pub use protocol::{
    DescriptorAnswer, DescriptorOffer, HandshakeComplete, HandshakeInit, NoiseHandshake,
    RelayComplete, RelayEnvelope, RelayForward, RelayRequest, RelayResponse,
};

// Re-export service types
pub use service::{
    EffectCommand, GuardDecision, GuardOutcome, GuardRequest, GuardSnapshot, RendezvousConfig,
    RendezvousService,
};

// Re-export descriptor types
pub use descriptor::{
    DescriptorBuilder, SelectedTransport, StunConfig, TransportProber, TransportSelector,
};

// Re-export channel types
pub use new_channel::{
    ChannelManager, ChannelState, HandshakeConfig, HandshakeResult, HandshakeStatus, Handshaker,
    SecureChannel,
};

// Re-export flood types
pub use flood::{DecryptedPayload, FloodPropagation, PacketBuilder, PacketCrypto, SeenNonces};

// Re-export LAN discovery types
pub use lan_discovery::{
    DiscoveredPeer, LanDiscoveryConfig, LanDiscoveryPacket, DEFAULT_ANNOUNCE_INTERVAL_MS,
    DEFAULT_LAN_PORT, MAX_PACKET_SIZE,
};
