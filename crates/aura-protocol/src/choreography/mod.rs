//! Choreographic Protocol Infrastructure
//!
//! This module contains choreographic protocol patterns and infrastructure using rumpsteak-aura.
//! Choreographies are global protocol specifications that automatically project to
//! local session types for each participant.
//!
//! ## Architecture
//!
//! - **crdt_sync**: CRDT synchronization message types and utilities (shared infrastructure)
//! - **epoch_management**: Generic epoch rotation coordination patterns
//! - **handler_bridge**: Clean trait abstractions for choreographic handlers
//!
//! ## Protocol Organization
//!
//! Domain-specific choreographies have been moved to their appropriate feature crates:
//! - **Snapshot protocols** → `aura-journal::choreography`
//! - **Anti-entropy protocols** → `aura-sync::choreography`
//!
//! CRDT synchronization types remain here (aura-protocol) to avoid circular dependencies,
//! but are re-exported by aura-sync for convenient access.
//!
//! This follows the 8-layer architecture by organizing shared infrastructure vs.
//! feature-specific implementations.

pub mod crdt_sync;
pub mod epoch_management;
pub mod handler_bridge;

// Re-export CRDT synchronization types
pub use crdt_sync::{CrdtOperation, CrdtSyncData, CrdtSyncRequest, CrdtSyncResponse, CrdtType};

// Re-export the clean handler bridge traits
pub use handler_bridge::{
    ChoreographicAdapter, ChoreographicEndpoint, ChoreographicHandler, DefaultEndpoint,
    SendGuardProfile,
};

// Re-export epoch management utilities
pub use epoch_management::{EpochConfig, EpochRotation, EpochRotationCoordinator, RotationStatus};

#[cfg(test)]
pub use handler_bridge::MockChoreographicAdapter;
