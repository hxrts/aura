//! # Aura Transport - Layer 2 Transport Types and Protocol Definitions
//!
//! This crate provides privacy-aware transport data types, protocol message definitions,
//! and relationship-scoped connection abstractions. It implements Layer 2 of Aura's
//! 8-layer architecture - Specification layer with types only, no implementations.
//!
//! ## Architecture Compliance
//!
//! - **Layer 2 (Specification)**: Types and protocol definitions only
//! - **NO Effect Handlers**: Effect implementations belong in Layer 3 (aura-effects)
//! - **NO Choreographic Coordination**: Multi-party coordination belongs in Layer 4 (aura-protocol)
//! - **Privacy-by-Design**: Privacy preservation integrated into core types
//!
//! ## Key Design Principles
//!
//! **Privacy-by-Design**: Privacy mechanisms integrated into core types, not bolted on
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
//! use aura_core::RelationshipId;
//!
//! // Privacy-aware envelope with relationship scoping
//! let message = b"Hello, world!".to_vec();
//! let relationship_context = RelationshipId::new([1u8; 32]);
//! let envelope = Envelope::new_scoped(message, relationship_context, None);
//!
//! // Transport configuration with built-in privacy levels
//! let config = TransportConfig {
//!     privacy_level: PrivacyLevel::RelationshipScoped,
//!     ..Default::default()
//! };
//! ```
//!
//! ## See Also
//!
//! - `aura-effects/src/transport/` - Effect handlers for transport operations
//! - `aura-protocol/src/transport/` - Choreographic transport coordination
//! - [`work/027.md`](../../work/027.md) - Complete transport layer redesign plan

// Internal module implementations

/// Core transport types with privacy-by-design
///
/// This module provides fundamental transport abstractions including envelopes,
/// configuration, and connection management, all with built-in privacy preservation.
pub mod types {
    pub mod config;
    pub mod connection;
    pub mod envelope;
}

/// AMP ratchet helpers
pub mod amp;

/// Privacy-aware peer management
///
/// This module provides peer discovery, information management, and privacy-preserving
/// selection algorithms that protect capability information and relationship contexts.
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
