//! Core Transport Layer
//!
//! This module contains the fundamental transport abstractions and implementations
//! that form the foundation of the Aura transport system.
//!
//! ## Module Organization
//!
//! - `traits` - Core transport trait definitions
//! - `connections` - Connection management and lifecycle
//! - `authentication` - Authenticated channels and device credentials
//! - `adapters` - Capability-driven messaging and session type adapters
//! - `factory` - Transport factory for creating implementations

pub mod traits;
pub mod connections;
pub mod authentication;
pub mod adapters;
pub mod factory;

// Re-export core traits
pub use traits::Transport;

// Re-export connection management
pub use connections::{
    Connection, ConnectionBuilder, ConnectionManager,
    BroadcastResult,
};

// Re-export authentication
pub use authentication::{
    AuthenticatedChannel, AuthenticatedTransport,
    DeviceCredentials, AuthenticationChallenge, AuthenticationResponse,
};

// Re-export adapters
pub use adapters::{
    CapabilityTransportAdapter, CapabilityTransport,
    CapabilityMessage, MessageContent, AuthenticatedMessage,
    CapabilityMessageHandler, TransportAdapterFactory,
};

// Re-export factory
pub use factory::*;