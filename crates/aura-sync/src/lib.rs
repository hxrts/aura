//! Aura Journal Synchronization
//!
//! This crate provides choreographic protocols for journal synchronization
//! across devices in the Aura threshold identity platform.
//!
//! # Architecture
//!
//! This crate implements journal synchronization choreographies:
//! - `G_sync` - Main journal synchronization choreography
//! - `anti_entropy` - Digest-based reconciliation for missing operations
//! - `peer_discovery` - Finding and connecting to sync peers
//!
//! # Design Principles
//!
//! - Uses choreographic programming for distributed sync coordination
//! - Integrates with the Aura MPST framework for capability guards and journal coupling
//! - Provides clean separation to avoid namespace conflicts (E0428 errors)
//! - Works with journal CRDT semantics for eventual consistency

#![warn(missing_docs)]
#![forbid(unsafe_code)]

/// Main journal synchronization choreography (G_sync)
pub mod journal_sync;

/// Anti-entropy protocols for digest-based reconciliation
pub mod anti_entropy;

/// Peer discovery and connection management
pub mod peer_discovery;

/// Snapshot helper utilities (writer fences, events)
pub mod snapshot;

/// Cache invalidation helpers
pub mod cache;

/// OTA upgrade orchestration helpers
pub mod ota;

/// Errors for sync operations
// errors module removed - use aura_core::AuraError directly

// Re-export core types
pub use aura_core::{AccountId, AuraError, AuraResult, Cap, DeviceId, Journal};

// Re-export MPST types
pub use aura_mpst::{
    AuraRuntime, CapabilityGuard, ExecutionContext, JournalAnnotation, MpstError, MpstResult,
};

// Maintenance helpers
pub use cache::CacheEpochFloors;
pub use ota::{UpgradeCoordinator, UpgradeReadiness};
pub use snapshot::{SnapshotManager, WriterFence};

// Error re-exports removed - use aura_core::AuraError directly
