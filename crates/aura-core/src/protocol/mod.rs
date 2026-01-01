//! Protocol Types - Layer 1
//!
//! Protocol-level types for version negotiation and capability tracking.
//! These types enable graceful degradation when peers run different versions.
//!
//! See `docs/004_distributed_systems_contract.md` for version compatibility
//! guarantees and migration policies.

pub mod versions;

pub use versions::{
    check_compatibility, ProtocolCapabilities, ProtocolCapability, VersionCompatibility,
    CURRENT_VERSION, MIN_SUPPORTED_VERSION,
};
