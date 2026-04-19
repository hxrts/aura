use crate::VerificationResult;
use serde_json::Value;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};

/// Cache for property verification results with LRU and TTL eviction.
#[derive(Debug)]
pub(crate) struct PropertyCache {
    /// Cached results by property hash and state hash.
    pub(crate) cache: HashMap<u64, CachedResult>,
    /// LRU ordering for eviction.
    access_order: VecDeque<u64>,
    /// Maximum cache size.
    pub(crate) max_size: usize,
    /// Time-to-live for cache entries in logical time units (0 = no TTL).
    ttl: u64,
}

/// Cached verification result.
#[derive(Debug, Clone)]
pub(crate) struct CachedResult {
    /// The verification result.
    pub(crate) result: VerificationResult,
    /// Access count for LRU.
    access_count: u64,
    /// Cache timestamp for TTL-based eviction (logical time units).
    cached_at_ms: u64,
}

impl PropertyCache {
    pub(crate) fn current_time_ms() -> u64 {
        // Deterministic monotonic clock for cache timestamps.
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        COUNTER.fetch_add(1, Ordering::Relaxed)
    }

    #[cfg(test)]
    pub(crate) fn new(max_size: usize) -> Self {
        Self::new_with_ttl(max_size, 0)
    }

    pub(crate) fn new_with_ttl(max_size: usize, ttl: u64) -> Self {
        Self {
            cache: HashMap::new(),
            access_order: VecDeque::new(),
            max_size,
            ttl,
        }
    }

    fn is_expired(&self, cached_at: u64) -> bool {
        if self.ttl == 0 {
            return false;
        }

        let current_time = Self::current_time_ms();
        current_time.saturating_sub(cached_at) > self.ttl
    }

    pub(crate) fn get(&mut self, key: u64) -> Option<&CachedResult> {
        let is_stale = self
            .cache
            .get(&key)
            .map(|result| self.is_expired(result.cached_at_ms))
            .unwrap_or(false);

        if is_stale {
            self.remove(&key);
            return None;
        }

        if let Some(result) = self.cache.get_mut(&key) {
            result.access_count = result.access_count.saturating_add(1);
            if let Some(position) = self
                .access_order
                .iter()
                .position(|candidate| *candidate == key)
            {
                self.access_order.remove(position);
            }
            self.access_order.push_back(key);
            Some(result)
        } else {
            None
        }
    }

    pub(crate) fn insert(&mut self, key: u64, result: VerificationResult) {
        self.evict_expired();

        while self.cache.len() >= self.max_size {
            if let Some(oldest_key) = self.access_order.pop_front() {
                self.cache.remove(&oldest_key);
            }
        }

        self.cache.insert(
            key,
            CachedResult {
                result,
                access_count: 1,
                cached_at_ms: Self::current_time_ms(),
            },
        );
        self.access_order.push_back(key);
    }

    pub(crate) fn get_statistics(&self) -> Value {
        let total_accesses = self
            .cache
            .values()
            .map(|entry| entry.access_count)
            .sum::<u64>();
        let hit_rate = if total_accesses > 0 {
            self.cache.len() as f64 / total_accesses as f64
        } else {
            0.0
        };

        serde_json::json!({
            "entries": self.cache.len(),
            "hit_rate": hit_rate,
            "max_size": self.max_size,
            "ttl": self.ttl,
        })
    }

    pub(crate) fn ttl(&self) -> u64 {
        self.ttl
    }

    fn evict_expired(&mut self) {
        if self.ttl == 0 {
            return;
        }

        let expired_keys = self
            .cache
            .iter()
            .filter(|(_, value)| self.is_expired(value.cached_at_ms))
            .map(|(key, _)| *key)
            .collect::<Vec<_>>();

        for key in expired_keys {
            self.remove(&key);
        }
    }

    fn remove(&mut self, key: &u64) {
        self.cache.remove(key);
        if let Some(position) = self
            .access_order
            .iter()
            .position(|candidate| candidate == key)
        {
            self.access_order.remove(position);
        }
    }
}
