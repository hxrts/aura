//! Layer 2: Privacy-Aware Transport Types
//!
//! Core transport data types with privacy-by-design principles.
//!
//! **Key Types**:
//! - **Envelope**: Encrypted message wrapper with context scope
//! - **FrameHeader**: Frame type and sequence info without metadata
//! - **ScopedEnvelope**: Context-scoped encryption per (authority, context) pair
//! - **ConnectionInfo**: Connection metadata with authority identifiers
//! - **PrivacyLevel**: Configuration for metadata leakage control
//!
//! **Design** (per docs/108_transport_and_information_flow.md):
//! - All types designed with privacy-first approach
//! - Context scoping: Content encrypted per (source authority, destination authority, context)
//! - Metadata minimization: Frame headers contain only protocol essentials
//! - Configuration: PrivacyLevel controls verbosity of metadata collection

pub mod config;
pub mod connection;
/// Endpoint types and configuration.
pub mod endpoint;
pub mod envelope;
pub mod ids;

// Public API - curated exports only
pub use config::{PrivacyLevel, TransportConfig};
pub use connection::{
    ConnectionCloseReason, ConnectionId, ConnectionInfo, ConnectionState, PrivacyContext,
    ScopedConnectionId,
};
pub use envelope::{Envelope, FrameHeader, FrameType, ScopedEnvelope};
pub use ids::{MessageId, SequenceNumber};
