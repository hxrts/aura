//! Aura Journal Synchronization - Effect System Implementation
//!
//! This crate provides effect-based journal synchronization for the Aura
//! threshold identity platform using the algebraic effects architecture.
//!
//! # Architecture
//!
//! This crate implements journal synchronization using effect handlers:
//! - `SyncService` - Main synchronization service using effect composition
//! - `AntiEntropyService` - Digest-based reconciliation service
//! - `snapshot` - Snapshot and maintenance utilities
//! - `cache` - Cache invalidation and maintenance
//!
//! # Design Principles
//!
//! - **Effect-Based**: Uses algebraic effects pattern for side-effect management
//! - **Composable**: Handlers can be mixed and matched for different environments
//! - **Testable**: Pure logic separated from side effects via effect interfaces
//! - **CRDT-First**: Built around journal CRDT semantics for eventual consistency

#![allow(missing_docs)]
#![forbid(unsafe_code)]

/// Main synchronization service using effect composition
pub mod sync_service;

/// Anti-entropy protocols for digest-based reconciliation
pub mod anti_entropy;

/// Snapshot helper utilities (writer fences, events)
pub mod snapshot;

/// Cache invalidation helpers
pub mod cache;

/// Journal synchronization choreography (G_sync)
pub mod journal_sync;

/// Peer discovery and connection management
pub mod peer_discovery;

/// OTA upgrade orchestration helpers
pub mod ota;

/// Maintenance events and upgrade coordination types
pub mod maintenance;

// Re-export core types
pub use aura_core::{AccountId, AuraError, AuraResult, DeviceId};

// Re-export protocol effect types
pub use aura_protocol::effects::{AntiEntropyConfig, BloomDigest, SyncEffects, SyncError};

// Maintenance helpers
pub use cache::CacheEpochFloors;
pub use maintenance::{
    AdminReplaced, CacheInvalidated, CacheKey, IdentityEpochFence, MaintenanceEvent, 
    SnapshotCompleted, SnapshotProposed, UpgradeActivated, UpgradeKind, UpgradeProposal,
};
pub use ota::{UpgradeCoordinator, UpgradeReadiness};
pub use snapshot::{SnapshotManager, WriterFence};
pub use sync_service::SyncService;
