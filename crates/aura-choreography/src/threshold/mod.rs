//! Journal choreographic threshold cryptography protocols
//!
//! This module provides choreographic implementations for Journal operations:
//! - Threshold unwrapping with M-of-N secret reconstruction
//! - Share contribution and accumulation
//! - Node rotation with epoch-based anti-replay protection
//! - FROST threshold signatures integrated with journal model
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

pub mod frost_signing;
pub mod rotation;
pub mod share_contribution;
pub mod threshold;

pub use frost_signing::{
    journal_frost_sign, JournalFrostConfig, JournalFrostResult, JournalFrostSigningChoreography,
};
pub use rotation::{
    journal_rotate_node, JournalNodeRotationChoreography, NodeRotationConfig, NodeRotationResult,
};
pub use share_contribution::{
    journal_collect_shares, JournalShareContributionChoreography, ShareCollectionResult,
    ShareContributionConfig,
};
pub use threshold::{
    journal_threshold_unwrap, JournalThresholdChoreography, ThresholdResult, ThresholdUnwrapConfig,
};
