//! Layer 2: Privacy-Aware Transport Types
//!
//! Core transport data types with privacy-by-design principles.
//!
//! **Key Types**:
//! - **Envelope**: Encrypted message wrapper with relationship scope
//! - **FrameHeader**: Frame type and sequence info without metadata
//! - **ScopedEnvelope**: Relationship-scoped encryption per (peer, context) pair
//! - **ConnectionInfo**: Anonymized connection metadata (no peer identifiers exposed)
//! - **PrivacyLevel**: Configuration for metadata leakage control
//!
//! **Design** (per docs/108_transport_and_information_flow.md):
//! - All types designed with privacy-first approach
//! - Relationship scoping: Content encrypted per (source, destination, context)
//! - Metadata minimization: Frame headers contain only protocol essentials
//! - Configuration: PrivacyLevel controls verbosity of metadata collection

pub mod config;
pub mod connection;
pub mod envelope;
pub mod endpoint;

#[cfg(test)]
mod tests;

// Public API - curated exports only
pub use config::{PrivacyLevel, TransportConfig};
pub use connection::{ConnectionId, ConnectionInfo, ConnectionState, ScopedConnectionId};
pub use envelope::{Envelope, FrameHeader, FrameType, ScopedEnvelope};
