//! Layer 4: Storage Coordination & Orchestration Handlers
//!
//! Multi-party storage coordination orchestrating capability-based access control,
//! handler composition, and distributed storage protocols with flow budget integration.
//!
//! **Coordinator Types**:
//! - **StorageAccessCoordinator**: Capability-based access control with authorization checks
//! - **ReplicationCoordinator**: Data replication and erasure coding across replicas
//!
//! **Integration** (per docs/003_information_flow_contract.md):
//! Storage operations flow through guard chain (CapGuard → FlowGuard) to enforce
//! authorization and flow budgets before any storage side effect.

/// Storage access control and capability coordination
pub mod access_coordinator;

/// Replication and erasure coding coordination
pub mod replication_coordinator;

// Re-export main coordination types
pub use access_coordinator::{AccessDecision, AccessRequest, StorageAccessCoordinator};
pub use replication_coordinator::{ReplicationCoordinator, ReplicationStrategy};
