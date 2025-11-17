//! Storage coordination and orchestration handlers
//!
//! This module provides Layer 4 (Orchestration) coordination for storage operations,
//! including capability-based access control, multi-handler composition, and
//! distributed storage protocols.

/// Storage access control and capability coordination
pub mod access_coordinator;

/// Multi-handler storage coordination
pub mod storage_coordinator;

/// Replication and erasure coding coordination
pub mod replication_coordinator;

// Re-export main coordination types
pub use access_coordinator::{AccessDecision, AccessRequest, StorageAccessCoordinator};
pub use replication_coordinator::{ReplicationCoordinator, ReplicationStrategy};
pub use storage_coordinator::{StorageCoordinator, StorageCoordinatorBuilder};
