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
//! 1. **Home peers** - Co-residents with highest mutual trust
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
