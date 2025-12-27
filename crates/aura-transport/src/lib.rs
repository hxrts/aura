//! # Aura Transport - Layer 2: Specification (Domain Crate)
//!
//! **Purpose**: Define P2P communication abstractions and transport semantics.
//!
//! This crate provides privacy-aware transport data types, protocol message definitions,
//! and context-scoped connection abstractions.
//!
//! # Architecture Constraints
//!
//! **Layer 2 depends only on aura-core** (foundation).
//! - ✅ Transport semantics and message types
//! - ✅ P2P communication abstractions
//! - ✅ Privacy-by-design transport types
//! - ✅ Protocol message specifications
//! - ❌ NO effect handler implementations (use NetworkEffects from aura-effects)
//! - ❌ NO multi-party coordination (that's aura-protocol)
//! - ❌ NO runtime composition (that's aura-composition/aura-agent)
//!
//! ## Key Design Principles
//!
//! **Privacy-by-Design**: Privacy mechanisms integrated into core types, not bolted on
//! **Authority-Centric**: Uses `AuthorityId` for cross-authority communication
//! **Context-Scoped**: Uses `ContextId` for relational context scoping
//! **Conciseness**: Every file <300 lines, focused implementations, no over-engineering
//! **Library Integration**: Designed for compatibility with rumpsteak-aura and mature networking libraries
//! **Clean Architecture**: Pure specification layer with curated API surface
//!
//! ## Usage
//!
//! ```rust
//! use aura_transport::{
//!     Envelope, TransportConfig, PrivacyLevel, ConnectionId,
//!     PeerInfo, PrivacyAwareSelectionCriteria,
//! };
//! use aura_core::identifiers::ContextId;
//!
//! // Privacy-aware envelope with context scoping
//! let message = b"Hello, world!".to_vec();
//! let context_id = ContextId::new_from_entropy([1u8; 32]);
//! let envelope = Envelope::new_scoped(message, context_id, None);
//!
//! // Transport configuration with built-in privacy levels
//! let config = TransportConfig {
//!     privacy_level: PrivacyLevel::ContextScoped,
//!     ..Default::default()
//! };
//! ```
//!
//! ## See Also
//!
//! - `aura-effects/src/transport/` - Effect handlers for transport operations
//! - `aura-agent/src/transport/` - Choreographic transport coordination
//! - `docs/108_transport_and_information_flow.md` - Transport architecture and privacy

// Internal module implementations

/// Core transport types with privacy-by-design
///
/// This module provides fundamental transport abstractions including envelopes,
/// configuration, and connection management, all with built-in privacy preservation.
pub mod types {
    pub mod config;
    pub mod connection;
    /// Endpoint address representation for transport layer.
    pub mod endpoint;
    pub mod envelope;
}

/// AMP types (clean - no domain dependencies)
pub mod amp;

/// Transport domain facts for state changes
pub mod facts;

/// Privacy-aware peer management
///
/// This module provides peer discovery, information management, and privacy-preserving
/// selection algorithms that protect capability information and context scopes.
pub mod peers {
    pub mod info;
    pub mod selection;
}

/// Transport protocol implementations
///
/// This module contains protocol-specific implementations for STUN, hole punching,
/// and WebSocket communication, all designed with privacy preservation in mind.
pub mod protocols {
    pub mod hole_punch;
    pub mod stun;
    pub mod websocket;
}

/// Context-aware transport for authority-centric model
pub mod context_transport;

/// Relay selection strategies for message forwarding
///
/// This module provides deterministic relay selection algorithms that
/// use social topology to choose relay nodes for message forwarding.
pub mod relay;

/// Transport layer message types and protocol definitions
///
/// This module contains message types for transport layer protocols including
/// social coordination messages, rendezvous protocol messages, and other transport
/// domain concerns that have been moved from higher layers following the architectural
/// refactoring plan.
pub mod messages;

// Re-export types from sub-modules
pub use types::config::TransportConfig;
pub use types::connection::{ConnectionId, ConnectionInfo, ConnectionState, ScopedConnectionId};
pub use types::envelope::{Envelope, FrameHeader, FrameType, PrivacyLevel, ScopedEnvelope};

// Re-export peers from sub-modules
pub use peers::info::{BlindedPeerCapabilities, PeerInfo, ScopedPeerMetrics};
pub use peers::selection::PrivacyAwareSelectionCriteria;

// Re-export protocols from sub-modules
pub use protocols::hole_punch::{HolePunchMessage, PunchConfig};
pub use protocols::stun::{StunAttribute, StunClass, StunConfig, StunMessage, StunMethod};
pub use protocols::websocket::WebSocketMessage;

// Re-export context transport types
pub use context_transport::{
    ContextTransportConfig, ContextTransportEndpoint, ContextTransportMessage,
    ContextTransportMetrics, ContextTransportSession, SessionControl, SessionState,
    TransportProtocol,
};

// Re-export message types
pub use messages::{
    AuthenticationPayload, HandshakeResult, HandshakeTranscript, PayloadKind, PskHandshakeConfig,
    RendezvousMessage, SocialMessage, SocialPayload, StorageCapabilityAnnouncement,
    TransportDescriptor, TransportKind, TransportOfferPayload,
};

// Re-export AMP types
pub use amp::{AmpError, AmpHeader, AmpRatchetState, RatchetDerivation};

// Re-export fact types
pub use facts::{TransportFact, TransportFactDelta, TransportFactReducer, TRANSPORT_FACT_TYPE_ID};

// Re-export relay types
pub use relay::{
    hash_relay_seed, partition_by_relationship, select_by_tiers, select_one_from_tier,
    DeterministicRandomSelector,
};
