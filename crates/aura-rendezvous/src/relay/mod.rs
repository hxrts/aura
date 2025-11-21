//! Relay Coordination and Capability-Based Routing
//!
//! This module implements relay coordination for message routing with
//! capability-based access control and privacy-preserving relay selection.

mod capabilities;
mod coordinator;
mod forwarding;
mod node;
pub mod selection;

// Re-export node types
pub use node::{
    AggregateStats, BandwidthLimits, ConnectivityType, LatencyProfile, LocationInfo,
    PerformanceSample, PrivacyFeatures, QosGuarantees, RelayCapabilities, RelayMetrics, RelayNode,
    RelayPerformance, RelayStatus, RelayStream, RelayTrustInfo, StorageCapabilities, StreamFlags,
    StreamState, TrustAttestation,
};

// Re-export selection types
pub use selection::{
    RelayCandidate, RelaySelectionConfig, RelaySelectionResult, RelaySelector, RelayType,
};

// Re-export from the main relay.rs file for now (to be refactored)
// These will eventually be properly extracted into coordinator.rs
pub use crate::relay as legacy_relay;
