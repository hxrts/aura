//! Transport Infrastructure
//!
//! This module contains supporting infrastructure for transport operations,
//! including message envelopes, presence management, and peer discovery.

pub mod envelope;
pub mod peer_discovery;
pub mod presence;

// Re-export infrastructure components
pub use envelope::*;
pub use peer_discovery::*;
pub use presence::*;