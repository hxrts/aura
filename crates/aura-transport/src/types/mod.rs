//! Privacy-Aware Core Transport Types
//!
//! This module provides essential transport data types with built-in privacy preservation.
//! All types are designed with privacy-by-design principles and relationship scoping.

pub mod config;
pub mod connection;
pub mod envelope;

#[cfg(test)]
mod tests;

// Public API - curated exports only
pub use config::{PrivacyLevel, TransportConfig};
pub use connection::{ConnectionId, ConnectionInfo, ConnectionState, ScopedConnectionId};
pub use envelope::{Envelope, FrameHeader, FrameType, ScopedEnvelope};
