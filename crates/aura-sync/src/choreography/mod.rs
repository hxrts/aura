//! Choreographic Protocols for Synchronization Operations
//!
//! This module contains choreographic protocol implementations that are specific
//! to synchronization and anti-entropy operations. Moving these protocols here
//! resolves circular dependencies and follows proper architectural layering.
//!
//! ## Architecture
//!
//! These protocols are now in Layer 5 (Feature/Protocol Implementation) where
//! they belong, rather than Layer 4 (Protocol Coordination).
//!
//! ## Protocols
//!
//! - `anti_entropy`: Digest-based state reconciliation for journal synchronization
//! - `journal`: Journal synchronization protocols and metadata
//! - `snapshot`: Coordinated garbage collection with threshold approval
//! - `tree_coordination`: Tree operations coordination using choreographic protocols
//! - `tree_sync`: Tree synchronization between journal replicas

pub mod anti_entropy;
pub mod journal;
pub mod snapshot;
pub mod tree_coordination;
pub mod tree_sync;

pub use anti_entropy::{
    bilateral_sync, execute_as_requester, execute_as_responder, AntiEntropyConfig,
    AntiEntropyResult, ChoreographyError as AntiEntropyChoreographyError,
};

pub use snapshot::{
    AbortReason, Cut, ProposalId, Snapshot, SnapshotError, SnapshotResult, ThresholdSnapshotConfig,
    ThresholdSnapshotCoordinator,
};
