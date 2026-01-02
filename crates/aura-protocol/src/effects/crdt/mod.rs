//! Layer 4: CRDT Coordination (Choreographies + Delivery)
//!
//! This module provides choreography-facing coordination for CRDT synchronization.
//! It intentionally does **not** define the core CRDT handlers; those live in
//! `aura-journal::crdt` as pure, local enforcement of semilattice laws.
//!
//! Use this module for multi-party coordination:
//! - `CrdtCoordinator` (protocol bridge) - requires `crdt-sync` feature
//! - Delivery guarantees and gossip configuration
//! - Execution helpers for choreography integration
//!
//! ## Feature Flags
//!
//! - `crdt-sync`: Enables `CrdtCoordinator` for choreography-based CRDT synchronization.
//!   This provides an alternative to the digest-based sync in aura-anti-entropy.
//!   Currently not wired into any protocol; enable if choreography-based CRDT sync is needed.

mod composition;
#[cfg(feature = "crdt-sync")]
mod coordinator;
mod delivery;
mod execution;

pub use composition::ComposedHandler;
#[cfg(feature = "crdt-sync")]
pub use coordinator::{
    increment_actor, max_counter, merge_vector_clocks, CrdtCoordinator, CrdtCoordinatorError,
};
pub use delivery::{DeliveryConfig, DeliveryEffect, DeliveryGuarantee, GossipStrategy, TopicId};
pub use execution::{execute_cv_sync, execute_delta_gossip, execute_op_broadcast};
