//! Layer 5: Relay Coordination - Capability-Based Routing & Selection
//!
//! Relay coordination for message routing with capability-based access control
//! and privacy-preserving relay selection (per docs/110_rendezvous.md).
//!
//! **Key Components**:
//! - **RelayNode**: Relay peer with capabilities, metrics, performance, trust info
//! - **RelaySelector**: Privacy-aware relay selection with QoS preferences
//! - **RelayCapabilities**: Bandwidth, storage, connectivity constraints
//!
//! **Design** (per docs/108_transport_and_information_flow.md):
//! Relays are selected based on capability tokens (Biscuit), performance metrics, and
//! privacy preferences. Enables privacy-respecting relay chains for off-path message delivery.

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

// Re-export from the main relay.rs file (pending refactor)
// These will eventually be properly extracted into coordinator.rs
pub use crate::relay as legacy_relay;
