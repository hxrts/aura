//! Integrated SBB System
//!
//! Complete integrated systems combining SBB components with capability awareness
//! and connection management.

pub mod capability_aware;
pub mod connection;
pub mod sbb_system;

pub use capability_aware::{
    CapabilityAwareSbbCoordinator, SbbFlowBudget, SbbForwardingPolicy, SbbRelationship,
    TrustStatistics,
};
pub use connection::{
    ConnectionConfig, ConnectionManager, ConnectionMethod, ConnectionResult, PunchResult,
    PunchSession, QuicConfig, StunClient, StunResult,
};
pub use sbb_system::{
    IntegratedSbbSystem, SbbConfig, SbbDiscoveryRequest, SbbDiscoveryResult, SbbSystemBuilder,
};
