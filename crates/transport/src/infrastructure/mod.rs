//! Transport Infrastructure
//!
//! This module contains supporting infrastructure for transport operations,
//! including message envelopes, presence management, and peer discovery.
//!
//! ## Components
//!
//! - `envelope` - SSB envelope structure with CID computation
//! - `peer_discovery` - Unified peer discovery and selection
//! - `presence` - Presence ticket management and verification

pub mod envelope;
pub mod peer_discovery;
/// Presence ticket management and verification
pub mod presence;

// Re-export infrastructure components
pub use envelope::*;
pub use peer_discovery::*;
pub use presence::*;
