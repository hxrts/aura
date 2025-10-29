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

pub mod adapters;
pub mod authentication;
pub mod connections;
pub mod factory;
pub mod traits;

// Re-export core traits
pub use traits::Transport;

// Re-export connection management
pub use connections::{BroadcastResult, Connection, ConnectionBuilder, ConnectionManager};

// Re-export authentication
pub use authentication::{
    AuthenticatedChannel, AuthenticatedTransport, AuthenticationChallenge, AuthenticationResponse,
    DeviceCredentials,
};

// Re-export adapters
pub use adapters::{
    AuthenticatedMessage, CapabilityMessage, CapabilityMessageHandler, CapabilityTransport,
    CapabilityTransportAdapter, MessageContent, TransportAdapterFactory,
};

// Re-export factory
pub use factory::*;
