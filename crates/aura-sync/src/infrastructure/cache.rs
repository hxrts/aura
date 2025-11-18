//! Cache management with epoch tracking
//!
//! Provides unified cache invalidation and epoch floor tracking for sync operations.
//! Integrates with maintenance events and OTA upgrade coordination.
//!
//! # Architecture
//!
//! The cache system tracks:
//! - Epoch floors per cache key for invalidation
//! - Cache freshness based on current identity epoch
//! - Integration with maintenance events for coordinated invalidation
//!
//! # Usage
//!
//! ```rust,no_run
//! use aura_sync::infrastructure::CacheManager;
//! use aura_core::tree::Epoch;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let mut cache = CacheManager::new();
//!
//! // Invalidate keys at epoch 10
//! cache.invalidate_keys(&["key1", "key2"], 10);
//!
//! // Check if key is fresh
//! let is_fresh = cache.is_fresh("key1", 10);
//! # Ok(())
//! # }
//! ```

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use aura_core::tree::Epoch as TreeEpoch;

// =============================================================================
// Cache Invalidation Events
// =============================================================================

/// Cache invalidation event
///
/// Records which keys should be invalidated and the minimum epoch
/// at which they become valid again.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheInvalidation {
    /// Cache keys to invalidate
    pub keys: Vec<String>,

    /// Minimum epoch for validity (epoch floor)
    pub epoch_floor: TreeEpoch,

    /// Optional reason for invalidation
    pub reason: Option<String>,
}

impl CacheInvalidation {
    /// Create a new cache invalidation event
    pub fn new(keys: Vec<String>, epoch_floor: TreeEpoch) -> Self {
        Self {
            keys,
            epoch_floor,
            reason: None,
        }
    }

    /// Create a cache invalidation with a reason
    pub fn with_reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = Some(reason.into());
        self
    }
}

// =============================================================================
// Cache Epoch Tracker
// =============================================================================

/// Tracks epoch floors for cache keys
///
/// Maintains the minimum epoch at which each cache key becomes valid.
/// This is used to determine whether cached data can be served or needs refresh.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEpochTracker {
    /// Per-key epoch floors
    floors: HashMap<String, TreeEpoch>,
}

impl CacheEpochTracker {
    /// Create a new cache epoch tracker
    pub fn new() -> Self {
        Self {
            floors: HashMap::new(),
        }
    }

    /// Apply a cache invalidation event
    ///
    /// Updates epoch floors for the specified keys, ensuring monotonicity.
    pub fn apply_invalidation(&mut self, invalidation: &CacheInvalidation) {
        for key in &invalidation.keys {
            let floor = self
                .floors
                .entry(key.clone())
                .or_insert(invalidation.epoch_floor);

            // Epoch floors are monotonic - only increase
            if invalidation.epoch_floor > *floor {
                *floor = invalidation.epoch_floor;
            }
        }
    }

    /// Check if a cache key is fresh at the given epoch
    ///
    /// Returns `true` if the current epoch is >= the epoch floor for the key,
    /// or if the key has no epoch floor (never invalidated).
    pub fn is_fresh(&self, key: &str, current_epoch: TreeEpoch) -> bool {
        match self.floors.get(key) {
            Some(&floor) => current_epoch >= floor,
            None => true, // No invalidation recorded
        }
    }

    /// Get the epoch floor for a key
    ///
    /// Returns `None` if the key has never been invalidated.
    pub fn epoch_floor(&self, key: &str) -> Option<TreeEpoch> {
        self.floors.get(key).copied()
    }

    /// Invalidate a single key at the specified epoch
    pub fn invalidate_key(&mut self, key: impl Into<String>, epoch_floor: TreeEpoch) {
        let key = key.into();
        let floor = self.floors.entry(key).or_insert(epoch_floor);
        if epoch_floor > *floor {
            *floor = epoch_floor;
        }
    }

    /// Invalidate multiple keys at the specified epoch
    pub fn invalidate_keys(&mut self, keys: &[impl AsRef<str>], epoch_floor: TreeEpoch) {
        for key in keys {
            self.invalidate_key(key.as_ref().to_string(), epoch_floor);
        }
    }

    /// Clear all epoch floor tracking
    pub fn clear(&mut self) {
        self.floors.clear();
    }

    /// Get the number of tracked keys
    pub fn tracked_keys(&self) -> usize {
        self.floors.len()
    }

    /// Get all tracked keys and their epoch floors
    pub fn all_floors(&self) -> impl Iterator<Item = (&str, TreeEpoch)> + '_ {
        self.floors.iter().map(|(k, v)| (k.as_str(), *v))
    }
}

impl Default for CacheEpochTracker {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Cache Manager
// =============================================================================

/// High-level cache manager with epoch tracking
///
/// Provides a unified interface for cache invalidation and freshness checking,
/// integrating with Aura's epoch-based identity system.
pub struct CacheManager {
    /// Epoch tracker for invalidation
    epoch_tracker: CacheEpochTracker,

    /// Optional cache statistics
    stats: CacheStatistics,
}

impl CacheManager {
    /// Create a new cache manager
    pub fn new() -> Self {
        Self {
            epoch_tracker: CacheEpochTracker::new(),
            stats: CacheStatistics::default(),
        }
    }

    /// Apply a cache invalidation event
    pub fn apply_invalidation(&mut self, invalidation: &CacheInvalidation) {
        self.epoch_tracker.apply_invalidation(invalidation);
        self.stats.total_invalidations += 1;
        self.stats.total_keys_invalidated += invalidation.keys.len();
    }

    /// Check if a key is fresh at the current epoch
    pub fn is_fresh(&self, key: &str, current_epoch: TreeEpoch) -> bool {
        let fresh = self.epoch_tracker.is_fresh(key, current_epoch);

        // Update statistics (in a real implementation, would use atomic counters)
        if fresh {
            // Would increment cache hits
        } else {
            // Would increment cache misses
        }

        fresh
    }

    /// Invalidate keys at the specified epoch
    pub fn invalidate_keys(&mut self, keys: &[impl AsRef<str>], epoch_floor: TreeEpoch) {
        self.epoch_tracker.invalidate_keys(keys, epoch_floor);
        self.stats.total_invalidations += 1;
        self.stats.total_keys_invalidated += keys.len();
    }

    /// Get epoch floor for a key
    pub fn epoch_floor(&self, key: &str) -> Option<TreeEpoch> {
        self.epoch_tracker.epoch_floor(key)
    }

    /// Get cache statistics
    pub fn statistics(&self) -> &CacheStatistics {
        &self.stats
    }

    /// Clear all cache state
    pub fn clear(&mut self) {
        self.epoch_tracker.clear();
        self.stats = CacheStatistics::default();
    }
}

impl Default for CacheManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Cache statistics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CacheStatistics {
    /// Total number of invalidation events processed
    pub total_invalidations: usize,

    /// Total number of keys invalidated
    pub total_keys_invalidated: usize,

    /// Number of currently tracked keys
    pub tracked_keys: usize,
}

// =============================================================================
// Legacy Compatibility
// =============================================================================

/// Legacy type alias for migration
///
/// This maintains backwards compatibility with existing code that uses
/// `CacheEpochFloors`. New code should use `CacheEpochTracker`.
#[deprecated(note = "Use CacheEpochTracker instead")]
pub type CacheEpochFloors = CacheEpochTracker;

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_floor_monotonic() {
        let mut tracker = CacheEpochTracker::new();

        // Apply invalidation at epoch 5
        tracker.apply_invalidation(&CacheInvalidation::new(vec!["foo".into()], 5_u64));

        // Key is not fresh before epoch 5
        assert!(!tracker.is_fresh("foo", 4_u64));

        // Key is fresh at epoch 5 and beyond
        assert!(tracker.is_fresh("foo", 5_u64));
        assert!(tracker.is_fresh("foo", 6_u64));

        // Higher floor overrides
        tracker.apply_invalidation(&CacheInvalidation::new(vec!["foo".into()], 7_u64));
        assert!(!tracker.is_fresh("foo", 6_u64));
        assert!(tracker.is_fresh("foo", 7_u64));
    }

    #[test]
    fn test_cache_floor_multiple_keys() {
        let mut tracker = CacheEpochTracker::new();

        tracker.invalidate_keys(&["key1", "key2", "key3"], 10_u64);

        assert!(!tracker.is_fresh("key1", 9_u64));
        assert!(tracker.is_fresh("key1", 10_u64));
        assert!(!tracker.is_fresh("key2", 9_u64));
        assert!(tracker.is_fresh("key2", 10_u64));
        assert!(!tracker.is_fresh("key3", 9_u64));
        assert!(tracker.is_fresh("key3", 10_u64));
    }

    #[test]
    fn test_cache_manager_statistics() {
        let mut manager = CacheManager::new();

        manager.invalidate_keys(&["a", "b", "c"], 5_u64);
        manager.invalidate_keys(&["d", "e"], 10_u64);

        let stats = manager.statistics();
        assert_eq!(stats.total_invalidations, 2);
        assert_eq!(stats.total_keys_invalidated, 5);
    }

    #[test]
    fn test_cache_invalidation_with_reason() {
        let invalidation = CacheInvalidation::new(vec!["test".into()], 5_u64)
            .with_reason("OTA upgrade to version 2.0");

        assert_eq!(
            invalidation.reason,
            Some("OTA upgrade to version 2.0".to_string())
        );
    }

    #[test]
    fn test_never_invalidated_key_is_fresh() {
        let tracker = CacheEpochTracker::new();

        // Keys that have never been invalidated are always fresh
        assert!(tracker.is_fresh("never_seen", 0_u64));
        assert!(tracker.is_fresh("never_seen", 100_u64));
    }
}
