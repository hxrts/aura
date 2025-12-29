//! Sync Metrics - Shared types for synchronization operations
//!
//! This module provides shared metric types for tree synchronization.
//! The full sync protocol traits and types are defined in `aura-protocol` and `aura-sync`.

use serde::{Deserialize, Serialize};

/// Metrics returned from a sync operation
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SyncMetrics {
    /// Number of operations applied during sync
    pub applied: u32,
    /// Number of duplicate operations skipped
    pub duplicates: u32,
    /// Number of sync rounds performed
    pub rounds: u32,
}

impl SyncMetrics {
    /// Create empty metrics
    pub fn empty() -> Self {
        Self::default()
    }

    /// Create metrics with just an applied count
    #[must_use]
    pub fn with_applied(applied: u32) -> Self {
        Self {
            applied,
            duplicates: 0,
            rounds: 1,
        }
    }
}
