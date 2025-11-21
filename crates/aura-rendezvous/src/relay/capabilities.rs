//! Relay Capabilities
//!
//! Capability definitions for relay nodes (extracted from node.rs).

// Re-exports are handled by node.rs (reserved for future use)
#[allow(unused_imports)]
pub use super::node::{
    BandwidthLimits, PrivacyFeatures, QosGuarantees, RelayCapabilities, StorageCapabilities,
};
