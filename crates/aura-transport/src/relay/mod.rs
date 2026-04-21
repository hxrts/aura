//! Relay Selection Module
//!
//! Provides relay selection strategies for message forwarding through
//! Aura's social topology.
//!
//! # Design
//!
//! Relay selection is deterministic for reproducibility in tests and simulation.
//! Selection uses `hash(domain, context_id, epoch, nonce)` to pick relays from
//! candidates in a tier-based priority order:
//!
//! 1. **Home peers** - Co-members with highest mutual trust
//! 2. **Neighborhood peers** - Adjacent home members with traversal rights
//! 3. **Guardians** - Fallback with explicit relay capability
//!
//! # Example
//!
//! ```ignore
//! use aura_transport::relay::DeterministicRandomSelector;
//! use aura_core::effects::relay::{RelaySelector, RelayContext, RelayCandidate};
//!
//! let selector = DeterministicRandomSelector::new(true); // prefer proximity
//! let relays = selector.select(&context, &candidates);
//! ```

mod deterministic;
mod helpers;

pub use deterministic::DeterministicRandomSelector;
pub use helpers::{
    hash_relay_seed, partition_by_relationship, select_by_tiers, select_one_from_tier,
};

#[cfg(test)]
pub(crate) mod test_support {
    use aura_core::{
        effects::relay::{RelayCandidate, RelayContext},
        types::identifiers::{AuthorityId, ContextId},
    };

    pub(crate) fn test_authority(seed: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([seed; 32])
    }

    pub(crate) fn test_context() -> RelayContext {
        RelayContext::new(
            ContextId::new_from_entropy([1u8; 32]),
            test_authority(1),
            test_authority(2),
            1,
            [0u8; 32],
        )
    }

    pub(crate) fn block_candidate(seed: u8) -> RelayCandidate {
        RelayCandidate::block_peer(test_authority(seed), [seed; 32])
    }

    pub(crate) fn neighborhood_candidate(seed: u8) -> RelayCandidate {
        RelayCandidate::neighborhood_hop_member(test_authority(seed), [seed; 32])
    }

    pub(crate) fn guardian_candidate(seed: u8) -> RelayCandidate {
        RelayCandidate::guardian(test_authority(seed))
    }
}
