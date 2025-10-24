//! Quota management and LRU eviction
//!
//! This module provides storage quota tracking and LRU-based cache eviction
//! to manage storage limits across accounts and peer caches.

use aura_journal::{AccountId, DeviceId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Quota configuration
///
/// Configuration parameters for storage quotas including per-account
/// and per-peer cache limits.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaConfig {
    /// Maximum bytes per account (default: 10 GiB)
    pub account_limit: u64,
    /// Maximum bytes per peer cache (default: 1 GiB)
    pub peer_cache_limit: u64,
}

impl Default for QuotaConfig {
    fn default() -> Self {
        QuotaConfig {
            account_limit: 10 * 1024 * 1024 * 1024, // 10 GiB
            peer_cache_limit: 1024 * 1024 * 1024,   // 1 GiB
        }
    }
}

/// Quota tracker
///
/// Tracks storage usage across accounts and peer caches, enforcing
/// quota limits and providing usage statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaTracker {
    /// Bytes pinned per account (permanent storage)
    pub pinned_bytes: BTreeMap<AccountId, u64>,
    /// Bytes cached per account (evictable storage)
    pub cached_bytes: BTreeMap<AccountId, u64>,
    /// Bytes cached per peer device
    pub peer_cache: BTreeMap<DeviceId, u64>,
    /// Quota configuration limits
    pub config: QuotaConfig,
}

impl QuotaTracker {
    /// Create new quota tracker with given configuration
    pub fn new(config: QuotaConfig) -> Self {
        QuotaTracker {
            pinned_bytes: BTreeMap::new(),
            cached_bytes: BTreeMap::new(),
            peer_cache: BTreeMap::new(),
            config,
        }
    }

    /// Check if account can store more data
    ///
    /// Returns true if adding `size` bytes would not exceed the account limit.
    pub fn can_store(&self, account_id: &AccountId, size: u64) -> bool {
        let current = self.pinned_bytes.get(account_id).copied().unwrap_or(0)
            + self.cached_bytes.get(account_id).copied().unwrap_or(0);
        current + size <= self.config.account_limit
    }

    /// Check if peer cache can store more data
    pub fn can_cache_peer(&self, device_id: &DeviceId, size: u64) -> bool {
        let current = self.peer_cache.get(device_id).copied().unwrap_or(0);
        current + size <= self.config.peer_cache_limit
    }

    /// Add pinned bytes
    pub fn add_pinned(&mut self, account_id: AccountId, size: u64) {
        *self.pinned_bytes.entry(account_id).or_insert(0) += size;
    }

    /// Add cached bytes
    pub fn add_cached(&mut self, account_id: AccountId, size: u64) {
        *self.cached_bytes.entry(account_id).or_insert(0) += size;
    }

    /// Add peer cache bytes
    pub fn add_peer_cache(&mut self, device_id: DeviceId, size: u64) {
        *self.peer_cache.entry(device_id).or_insert(0) += size;
    }

    /// Remove pinned bytes
    pub fn remove_pinned(&mut self, account_id: &AccountId, size: u64) {
        if let Some(current) = self.pinned_bytes.get_mut(account_id) {
            *current = current.saturating_sub(size);
        }
    }

    /// Remove cached bytes
    pub fn remove_cached(&mut self, account_id: &AccountId, size: u64) {
        if let Some(current) = self.cached_bytes.get_mut(account_id) {
            *current = current.saturating_sub(size);
        }
    }

    /// Get total usage for account
    pub fn get_usage(&self, account_id: &AccountId) -> u64 {
        self.pinned_bytes.get(account_id).copied().unwrap_or(0)
            + self.cached_bytes.get(account_id).copied().unwrap_or(0)
    }
}

/// LRU cache entry
///
/// Represents a cached item with metadata for LRU eviction decisions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEntry {
    /// Content identifier of the cached item
    pub cid: aura_journal::Cid,
    /// Size of the cached item in bytes
    pub size: u64,
    /// Unix timestamp of last access
    pub last_access: u64,
}

/// LRU eviction policy
///
/// Implements least-recently-used eviction for cache management.
/// Tracks access patterns and provides eviction candidates when storage pressure occurs.
pub struct LruEviction {
    /// Ordered list of cache entries (oldest first)
    entries: Vec<CacheEntry>,
}

impl LruEviction {
    /// Create new LRU eviction tracker
    pub fn new() -> Self {
        LruEviction {
            entries: Vec::new(),
        }
    }

    /// Record access
    pub fn access(&mut self, cid: aura_journal::Cid, size: u64, effects: &aura_crypto::Effects) {
        // Remove if exists
        self.entries.retain(|e| e.cid.0 != cid.0);

        // Add to end (most recently used)
        self.entries.push(CacheEntry {
            cid,
            size,
            last_access: current_timestamp_with_effects(effects),
        });
    }

    /// Get entries to evict (least recently used first)
    pub fn get_eviction_candidates(&self, target_bytes: u64) -> Vec<aura_journal::Cid> {
        let mut evict = Vec::new();
        let mut freed = 0u64;

        // Iterate from oldest to newest
        for entry in &self.entries {
            if freed >= target_bytes {
                break;
            }
            evict.push(entry.cid.clone());
            freed += entry.size;
        }

        evict
    }

    /// Remove entries
    pub fn remove(&mut self, cids: &[aura_journal::Cid]) {
        self.entries
            .retain(|e| !cids.iter().any(|c| c.0 == e.cid.0));
    }
}

impl Default for LruEviction {
    fn default() -> Self {
        Self::new()
    }
}

/// Get current timestamp using injected effects
fn current_timestamp_with_effects(effects: &aura_crypto::Effects) -> u64 {
    effects.now().unwrap_or(0)
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quota_tracking() {
        let effects = aura_crypto::Effects::for_test("test_quota_tracking");
        let config = QuotaConfig {
            account_limit: 1000,
            peer_cache_limit: 500,
        };
        let mut tracker = QuotaTracker::new(config);

        let account_id = AccountId::new_with_effects(&effects);

        assert!(tracker.can_store(&account_id, 500));
        tracker.add_pinned(account_id, 500);
        assert_eq!(tracker.get_usage(&account_id), 500);

        assert!(tracker.can_store(&account_id, 400));
        assert!(!tracker.can_store(&account_id, 600));
    }

    #[test]
    fn test_lru_eviction() {
        let effects = aura_crypto::Effects::for_test("test_lru_eviction");
        let mut lru = LruEviction::new();

        lru.access(aura_journal::Cid("a".to_string()), 100, &effects);
        lru.access(aura_journal::Cid("b".to_string()), 100, &effects);
        lru.access(aura_journal::Cid("c".to_string()), 100, &effects);

        let candidates = lru.get_eviction_candidates(150);
        assert_eq!(candidates.len(), 2); // Should evict 'a' and 'b'
        assert_eq!(candidates[0].0, "a");
        assert_eq!(candidates[1].0, "b");
    }
}
