//! Privacy-Preserving Peer Management
//!
//! This module provides peer management types with built-in privacy preservation,
//! capability blinding, and relationship-scoped discovery mechanisms.

pub mod info;
pub mod selection;

#[cfg(test)]
mod tests;

// Public API - curated exports only
pub use info::{PeerInfo, BlindedPeerCapabilities, ScopedPeerMetrics};
pub use selection::{PrivacyAwareSelectionCriteria, RelationshipScopedDiscovery};