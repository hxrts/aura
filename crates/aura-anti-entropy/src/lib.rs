#![deny(clippy::dbg_macro)]
#![deny(clippy::todo)]
#![allow(
    missing_docs,
    unused_variables,
    clippy::unwrap_used,
    clippy::expect_used,
    dead_code,
    clippy::match_like_matches_macro,
    clippy::type_complexity,
    clippy::while_let_loop,
    clippy::redundant_closure,
    clippy::large_enum_variant,
    clippy::unused_unit,
    clippy::get_first,
    clippy::single_range_in_vec_init,
    clippy::disallowed_methods,
    deprecated
)]
//! # Aura Anti-Entropy - Layer 4: Sync and Reconciliation
//!
//! This module provides all synchronization functionality for Aura including:
//! - Effect traits for sync operations
//! - Persistent sync handler backed by storage (production)
//! - Anti-entropy handler for digest-based reconciliation
//! - Broadcaster handler for eager push operations
//!
//! ## Design Principles
//!
//! - **Digest Exchange**: Bloom filters or rolling hashes for efficient comparison
//! - **Bounded Leakage**: Rate limiting and batching for privacy
//! - **Pull-Based**: Requestor drives sync (no unsolicited pushes)
//! - **Verification**: All received operations verified before storage
//! - **Shared Storage**: Sync and tree handlers share the same storage backend

pub mod anti_entropy;
pub mod broadcast;
pub mod config;
pub mod effects;
pub mod persistent;
pub mod prelude;
pub mod pure;
pub mod wire;

// Re-export effect types
pub use effects::{AntiEntropyConfig, BloomDigest, SyncEffects, SyncError, SyncMetrics};

// Re-export WakeCondition from aura_core for convenience
pub use aura_core::effects::WakeCondition;

// Re-export handler types
pub use anti_entropy::AntiEntropyHandler;
pub use broadcast::{BroadcastConfig, BroadcasterHandler};
pub use persistent::PersistentSyncHandler;

// Re-export storage constants for shared access
pub use aura_journal::commitment_tree::storage::{TREE_OPS_INDEX_KEY, TREE_OPS_PREFIX};
