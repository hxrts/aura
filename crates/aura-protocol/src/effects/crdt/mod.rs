//! Layer 4: CRDT Coordination (Choreographies + Delivery)
//!
//! This module provides choreography-facing coordination for CRDT synchronization.
//! It intentionally does **not** define the core CRDT handlers; those live in
//! `aura-journal::crdt` as pure, local enforcement of semilattice laws.
//!
//! Use this module for multi-party coordination:
//! - `CrdtCoordinator` (protocol bridge)
//! - Delivery guarantees and gossip configuration
//! - Execution helpers for choreography integration

mod composition;
mod crdt_coordinator;
mod delivery;
mod execution;

pub use composition::ComposedHandler;
pub use crdt_coordinator::{CrdtCoordinator, CrdtCoordinatorError};
pub use delivery::{DeliveryConfig, DeliveryEffect, DeliveryGuarantee, GossipStrategy, TopicId};
pub use execution::{execute_cv_sync, execute_delta_gossip, execute_op_broadcast};
