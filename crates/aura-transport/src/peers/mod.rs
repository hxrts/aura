//! Layer 2: Privacy-Aware Peer Management
//!
//! Peer discovery and management with privacy preservation: **PeerInfo** (anonymized peer metadata),
//! **BlindedPeerCapabilities** (privacy-preserving capability revelation), **PrivacyAwareSelectionCriteria**.
//!
//! **Design** (per docs/109_transport_and_information_flow.md):
//! - Context-scoped peer discovery: Peers visible only within specific relational contexts
//! - Capability blinding: Hide sensitive peer capabilities from observers
//! - Metadata minimization: Expose only protocol-necessary information
//! - Selection criteria: Privacy-aware peer selection for rendezvous and sync operations

pub mod info;
pub mod selection;

// Public API - curated exports only
pub use info::{BlindedPeerCapabilities, PeerInfo, ReliabilityLevel, ScopedPeerMetrics};
pub use selection::PrivacyAwareSelectionCriteria;
