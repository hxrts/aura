//! Pluggable transport layer with presence ticket enforcement
//!
//! This crate provides a modular transport system for Aura, organized into several
//! logical layers:
//!
//! ## Core Layer
//! - `core::traits` - Fundamental transport trait definitions
//! - `core::connections` - Connection management and lifecycle
//! - `core::authentication` - Authenticated channels and device credentials
//! - `core::adapters` - Capability-driven messaging and session type adapters
//! - `core::factory` - Transport factory for creating implementations
//!
//! ## Adapter Layer
//! - `adapters::memory` - In-memory transport for testing
//! - `adapters::https_relay` - HTTPS relay transport
//! - `adapters::noise_tcp` - Noise-encrypted TCP transport
//! - `adapters::simple_tcp` - Simple TCP transport (testing only)
//!
//! ## Infrastructure Layer
//! - `infrastructure::presence` - Presence ticket management
//! - `infrastructure::envelope` - Message envelope handling
//! - `infrastructure::peer_discovery` - Peer discovery mechanisms
//!
//! ## SSB Layer
//! - `ssb::publisher` - SSB envelope publishing
//! - `ssb::recognizer` - SSB envelope recognition
//! - `ssb::gossip` - SSB gossip protocol integration

// Core modules
pub mod error;
pub mod types;
pub mod core;

// Feature modules
pub mod adapters;
pub mod infrastructure;
pub mod session_types;
pub mod ssb;

// Re-export core types and traits for easy access
pub use error::{TransportError, TransportErrorBuilder, TransportResult};
pub use types::{PresenceTicket, TransportMessage};

// Re-export core transport abstractions
pub use core::{
    // Core traits
    Transport,
    // Connection management
    Connection, ConnectionBuilder, ConnectionManager, BroadcastResult,
    // Authentication
    AuthenticatedChannel, AuthenticatedTransport, DeviceCredentials,
    AuthenticationChallenge, AuthenticationResponse,
    // Adapters
    CapabilityTransportAdapter, CapabilityTransport, CapabilityMessage,
    MessageContent, AuthenticatedMessage, TransportAdapterFactory,
    // Factory
    TransportFactory, TransportConfig, TransportConfigBuilder,
    CapabilityConfig, AnyTransport,
};

// Re-export common adapter implementations
pub use adapters::{
    MemoryTransport,
    HttpsRelayTransport,
    NoiseTcpTransport, NoiseTcpTransportBuilder,
    SimpleTcpTransport, SimpleTcpTransportBuilder,
};

// Re-export infrastructure components
pub use infrastructure::{
    MessageEnvelope, EnvelopeBuilder,
    PeerDiscovery, DiscoveryEvent,
    PresenceManager, PresenceState,
};

// Re-export SSB components
pub use ssb::{
    SsbPublisher, SsbRecognizer, SsbGossip,
};

// Re-export session types
pub use session_types::*;