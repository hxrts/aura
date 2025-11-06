//! KeyJournal choreographic threshold cryptography protocols
//!
//! This module provides choreographic implementations for KeyJournal operations:
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

pub mod keyjournal_threshold;
pub mod keyjournal_share_contribution;
pub mod keyjournal_rotation;
pub mod keyjournal_frost_signing;

pub use keyjournal_threshold::{
    KeyJournalThresholdChoreography, ThresholdResult, ThresholdUnwrapConfig,
    keyjournal_threshold_unwrap,
};
pub use keyjournal_share_contribution::{
    KeyJournalShareContributionChoreography, ShareCollectionResult, ShareContributionConfig,
    keyjournal_collect_shares,
};
pub use keyjournal_rotation::{
    KeyJournalNodeRotationChoreography, NodeRotationResult, NodeRotationConfig,
    keyjournal_rotate_node,
};
pub use keyjournal_frost_signing::{
    KeyJournalFrostSigningChoreography, KeyJournalFrostResult, KeyJournalFrostConfig,
    keyjournal_frost_sign,
};
