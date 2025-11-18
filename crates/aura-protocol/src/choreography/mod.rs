//! Choreographic Protocol Infrastructure
//!
//! This module contains choreographic protocol patterns and infrastructure using rumpsteak-aura.
//! Choreographies are global protocol specifications that automatically project to
//! local session types for each participant.
//!
//! ## Architecture
//!
//! - **crdt_sync**: CRDT synchronization message types and utilities (shared infrastructure)
//! - **handler_bridge**: Clean trait abstractions for choreographic handlers
//!
//! ## Protocol Organization
//!
//! Domain-specific choreographies have been moved to their appropriate feature crates:
//! - **Snapshot protocols** → `aura-journal::choreography`
//! - **Anti-entropy protocols** → `aura-sync::choreography`
//! - **Epoch management** → `aura-sync::protocols`
//!
//! CRDT synchronization types remain here (aura-protocol) to avoid circular dependencies,
//! but are re-exported by aura-sync for convenient access.
//!
//! This follows the 8-layer architecture by organizing shared infrastructure vs.
//! feature-specific implementations.

pub mod crdt_sync;
// pub mod handler_bridge; // Disabled - needs Capability type rewrite

// Re-export CRDT synchronization types
pub use crdt_sync::{CrdtOperation, CrdtSyncData, CrdtSyncRequest, CrdtSyncResponse, CrdtType};

// Re-export the clean handler bridge traits (temporarily disabled)
// pub use handler_bridge::{
//     ChoreographicAdapter, ChoreographicEndpoint, ChoreographicHandler, DefaultEndpoint,
//     SendGuardProfile,
// };

// NOTE: Epoch management has been moved to aura-sync (Layer 5)
// Import aura-sync directly if you need epoch coordination protocols

// #[cfg(test)]
// pub use handler_bridge::MockChoreographicAdapter;
