//! Sync Module - Consolidated State Synchronization
//!
//! This module provides all synchronization functionality for Aura including:
//! - Effect traits for sync operations
//! - Local sync handler for single-node operations
//! - Anti-entropy handler for digest-based reconciliation
//! - Broadcaster handler for eager push operations
//!
//! ## Design Principles
//!
//! - **Digest Exchange**: Bloom filters or rolling hashes for efficient comparison
//! - **Bounded Leakage**: Rate limiting and batching for privacy
//! - **Pull-Based**: Requestor drives sync (no unsolicited pushes)
//! - **Verification**: All received operations verified before storage

pub mod anti_entropy;
pub mod broadcaster;
pub mod effects;
pub mod local;

// Re-export effect types
pub use effects::{AntiEntropyConfig, BloomDigest, SyncEffects, SyncError};

// Re-export core sync metrics from aura_core
pub use effects::SyncMetrics;

// Re-export WakeCondition from aura_core for convenience
pub use aura_core::effects::WakeCondition;

// Re-export handler types
pub use anti_entropy::AntiEntropyHandler;
pub use broadcaster::{BroadcastConfig, BroadcasterHandler};
pub use local::LocalSyncHandler;
