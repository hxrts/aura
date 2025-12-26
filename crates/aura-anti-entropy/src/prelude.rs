//! Aura anti-entropy prelude.
//!
//! Curated re-exports for sync orchestration.

pub use crate::sync::{
    AntiEntropyConfig, AntiEntropyHandler, BloomDigest, BroadcastConfig, BroadcasterHandler,
    PersistentSyncHandler, SyncEffects, SyncError, SyncMetrics,
};
pub use crate::sync::anti_entropy::AntiEntropyProtocolEffects;
