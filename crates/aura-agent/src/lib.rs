//! Aura Agent: Middleware-based identity and session management
//!
//! This crate provides a middleware-based agent implementation following
//! the foundation pattern established in the journal layer.

// Core middleware system
pub mod middleware;

// High-level KeyFabric agent API
pub mod keyfabric_agent;

// Re-export middleware components
pub use middleware::*;

// Re-export KeyFabric agent
pub use keyfabric_agent::*;

// Keep existing platform secure storage and utilities
pub mod device_secure_store;
pub mod utils;
pub mod error;

// Re-export essential types
pub use error::{AgentError, Result, ResultExt};
pub use device_secure_store::{
    DeviceAttestation, PlatformSecureStorage, SecureStorage, SecurityLevel,
};

// Re-export typed identifiers from aura-types
pub use aura_types::{DataId, CapabilityId, DeviceId};