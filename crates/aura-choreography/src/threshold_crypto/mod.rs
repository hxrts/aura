//! KeyFabric choreographic threshold cryptography protocols
//!
//! This module provides choreographic implementations for KeyFabric operations:
//! - Threshold unwrapping with M-of-N secret reconstruction  
//! - Share contribution and accumulation
//! - Node rotation with epoch-based anti-replay protection
//! - FROST threshold signatures integrated with fabric model
//!
//! ## Pattern-Based Implementations
//!
//! All implementations use the fundamental choreographic patterns:
//! - propose_and_acknowledge for initialization and coordination
//! - broadcast_and_gather for information exchange
//! - verify_consistent_result for result verification
//!
//! This provides:
//! - Enhanced Byzantine tolerance
//! - Consistent security properties
//! - Uniform timeout and error handling
//! - Simplified development of new threshold protocols

pub mod keyfabric_threshold;
pub mod keyfabric_share_contribution;
pub mod keyfabric_rotation;
pub mod keyfabric_frost_signing;

pub use keyfabric_threshold::{
    KeyFabricThresholdChoreography, ThresholdResult, ThresholdUnwrapConfig,
    keyfabric_threshold_unwrap,
};
pub use keyfabric_share_contribution::{
    KeyFabricShareContributionChoreography, ShareCollectionResult, ShareContributionConfig,
    keyfabric_collect_shares,
};
pub use keyfabric_rotation::{
    KeyFabricNodeRotationChoreography, NodeRotationResult, NodeRotationConfig,
    keyfabric_rotate_node,
};
pub use keyfabric_frost_signing::{
    KeyFabricFrostSigningChoreography, KeyFabricFrostResult, KeyFabricFrostConfig,
    keyfabric_frost_sign,
};
