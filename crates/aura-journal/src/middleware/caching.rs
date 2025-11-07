//! Caching middleware for performance optimization

use super::{JournalContext, JournalHandler, JournalMiddleware};
use crate::error::{Error, Result};
use crate::operations::JournalOperation;
use aura_types::effects::TimeEffects;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Duration;

/// Caching middleware that caches operation results
pub struct CachingMiddleware<T: TimeEffects> {
    /// Cache storage
    cache: Arc<RwLock<Cache>>,

    /// Configuration
    config: CachingConfig,

    /// Time effects for TTL management
    time_effects: Arc<T>,
}

impl<T: TimeEffects> CachingMiddleware<T> {
    /// Create new caching middleware with time effects
    pub fn new(config: CachingConfig, time_effects: Arc<T>) -> Self {
        Self {
            cache: Arc::new(RwLock::new(Cache::new())),
            config,
            time_effects,
        }
    }

    /// Get cache statistics
    pub fn stats(&self) -> CacheStats {
        self.cache.read().unwrap().stats()
    }

    /// Clear all cached entries
    pub fn clear(&self) {
        self.cache.write().unwrap().clear();
    }
}

impl<T: TimeEffects> JournalMiddleware for CachingMiddleware<T> {
    fn process(
        &self,
        operation: JournalOperation,
        context: &JournalContext,
        next: &dyn JournalHandler,
    ) -> Result<serde_json::Value> {
        // Skip caching if disabled
        if !self.config.enable_caching {
            return next.handle(operation, context);
        }

        // Check if operation is cacheable
        if !self.is_cacheable(&operation) {
            return next.handle(operation, context);
        }

        // Generate cache key
        let cache_key = self.generate_cache_key(&operation, context);

        // Try to get from cache first
        if let Some(cached_result) = self.get_from_cache(&cache_key)? {
            return Ok(cached_result);
        }

        // Cache miss - execute operation
        let result = next.handle(operation, context)?;

        // Cache the result if successful
        self.put_in_cache(cache_key, result.clone())?;

        Ok(result)
    }

    fn name(&self) -> &str {
        "caching"
    }
}

impl<T: TimeEffects> CachingMiddleware<T> {
    fn is_cacheable(&self, operation: &JournalOperation) -> bool {
        match operation {
            // Read operations are cacheable
            JournalOperation::GetDevices => true,
            JournalOperation::GetEpoch => true,

            // Write operations are not cacheable
            JournalOperation::AddDevice { .. } => false,
            JournalOperation::RemoveDevice { .. } => false,
            JournalOperation::AddGuardian { .. } => false,
            JournalOperation::IncrementEpoch => false,
        }
    }

    fn generate_cache_key(&self, operation: &JournalOperation, context: &JournalContext) -> String {
        format!(
            "{}:{}:{:?}",
            context.account_id.to_string(),
            context.operation_type,
            operation
        )
    }

    fn get_from_cache(&self, key: &str) -> Result<Option<serde_json::Value>> {
        let now_millis = self
            .time_effects
            .now_millis()
            .map_err(|e| Error::storage_failed(&format!("Failed to get time: {:?}", e)))?;

        let mut cache = self
            .cache
            .write()
            .map_err(|_| Error::storage_failed("Failed to acquire write lock on cache"))?;

        Ok(cache.get(key, now_millis))
    }

    fn put_in_cache(&self, key: String, value: serde_json::Value) -> Result<()> {
        let now_millis = self
            .time_effects
            .now_millis()
            .map_err(|e| Error::storage_failed(&format!("Failed to get time: {:?}", e)))?;

        let mut cache = self
            .cache
            .write()
            .map_err(|_| Error::storage_failed("Failed to acquire write lock on cache"))?;

        cache.put(key, value, self.config.default_ttl, now_millis);
        Ok(())
    }
}

/// Configuration for caching middleware
#[derive(Debug, Clone)]
pub struct CachingConfig {
    /// Whether caching is enabled
    pub enable_caching: bool,

    /// Default time-to-live for cached entries
    pub default_ttl: Duration,

    /// Maximum number of cached entries
    pub max_entries: usize,

    /// Whether to cache read operations
    pub cache_reads: bool,

    /// Whether to cache write operation results
    pub cache_writes: bool,
}

impl Default for CachingConfig {
    fn default() -> Self {
        Self {
            enable_caching: true,
            default_ttl: Duration::from_secs(300), // 5 minutes
            max_entries: 1000,
            cache_reads: true,
            cache_writes: false, // Write results typically shouldn't be cached
        }
    }
}

/// Cache entry with expiration
#[derive(Debug, Clone)]
struct CacheEntry {
    value: serde_json::Value,
    expires_at_millis: u64,
    created_at_millis: u64,
}

impl CacheEntry {
    fn new(value: serde_json::Value, ttl: Duration, now_millis: u64) -> Self {
        Self {
            value,
            expires_at_millis: now_millis + ttl.as_millis() as u64,
            created_at_millis: now_millis,
        }
    }

    fn is_expired(&self, now_millis: u64) -> bool {
        now_millis > self.expires_at_millis
    }
}

/// Simple in-memory cache implementation
struct Cache {
    entries: HashMap<String, CacheEntry>,
    hits: u64,
    misses: u64,
    evictions: u64,
}

impl Cache {
    fn new() -> Self {
        Self {
            entries: HashMap::new(),
            hits: 0,
            misses: 0,
            evictions: 0,
        }
    }

    fn get(&mut self, key: &str, now_millis: u64) -> Option<serde_json::Value> {
        // Clean up expired entries periodically
        if self.entries.len() % 100 == 0 {
            self.cleanup_expired(now_millis);
        }

        if let Some(entry) = self.entries.get(key) {
            if entry.is_expired(now_millis) {
                self.entries.remove(key);
                self.misses += 1;
                None
            } else {
                self.hits += 1;
                Some(entry.value.clone())
            }
        } else {
            self.misses += 1;
            None
        }
    }

    fn put(&mut self, key: String, value: serde_json::Value, ttl: Duration, now_millis: u64) {
        // Evict if at capacity
        if self.entries.len() >= 1000 {
            // TODO: make configurable
            self.evict_oldest();
        }

        let entry = CacheEntry::new(value, ttl, now_millis);
        self.entries.insert(key, entry);
    }

    fn clear(&mut self) {
        self.entries.clear();
        self.hits = 0;
        self.misses = 0;
        self.evictions = 0;
    }

    fn stats(&self) -> CacheStats {
        CacheStats {
            entries: self.entries.len(),
            hits: self.hits,
            misses: self.misses,
            evictions: self.evictions,
            hit_rate: if self.hits + self.misses > 0 {
                self.hits as f64 / (self.hits + self.misses) as f64
            } else {
                0.0
            },
        }
    }

    fn cleanup_expired(&mut self, now_millis: u64) {
        self.entries
            .retain(|_, entry| now_millis <= entry.expires_at_millis);
    }

    fn evict_oldest(&mut self) {
        if let Some(oldest_key) = self
            .entries
            .iter()
            .min_by_key(|(_, entry)| entry.created_at_millis)
            .map(|(key, _)| key.clone())
        {
            self.entries.remove(&oldest_key);
            self.evictions += 1;
        }
    }
}

/// Cache statistics
#[derive(Debug, Clone)]
pub struct CacheStats {
    /// Number of cached entries
    pub entries: usize,

    /// Number of cache hits
    pub hits: u64,

    /// Number of cache misses
    pub misses: u64,

    /// Number of evictions
    pub evictions: u64,

    /// Cache hit rate (0.0 to 1.0)
    pub hit_rate: f64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::middleware::handler::NoOpHandler;
    use crate::operations::JournalOperation;
    use aura_crypto::Effects;
    use aura_types::{AccountIdExt, DeviceIdExt};
    use std::time::Duration;

    #[test]
    fn test_caching_middleware_cache_hit() {
        let effects = Effects::test(42);
        let account_id = aura_types::AccountId::new_with_effects(&effects);
        let device_id = aura_types::DeviceId::new_with_effects(&effects);

        let time_effects = Arc::new(aura_types::effects::SystemTimeEffects::new());
        let middleware = CachingMiddleware::new(CachingConfig::default(), time_effects);
        let handler = NoOpHandler;
        let context = JournalContext::new(account_id, device_id, "test".to_string());
        let operation = JournalOperation::GetEpoch;

        // First call - cache miss
        let result1 = middleware.process(operation.clone(), &context, &handler);
        assert!(result1.is_ok());

        // Second call - should be cache hit
        let result2 = middleware.process(operation, &context, &handler);
        assert!(result2.is_ok());
        assert_eq!(result1.unwrap(), result2.unwrap());

        let stats = middleware.stats();
        assert_eq!(stats.hits, 1);
        assert_eq!(stats.misses, 1);
    }

    #[test]
    fn test_caching_middleware_non_cacheable_operation() {
        let effects = Effects::test(42);
        let account_id = aura_types::AccountId::new_with_effects(&effects);
        let device_id = aura_types::DeviceId::new_with_effects(&effects);

        let time_effects = Arc::new(aura_types::effects::SystemTimeEffects::new());
        let middleware = CachingMiddleware::new(CachingConfig::default(), time_effects);
        let handler = NoOpHandler;
        let context = JournalContext::new(account_id, device_id, "test".to_string());
        let operation = JournalOperation::IncrementEpoch; // Write operation - not cacheable

        let result = middleware.process(operation, &context, &handler);
        assert!(result.is_ok());

        let stats = middleware.stats();
        assert_eq!(stats.hits, 0);
        assert_eq!(stats.misses, 0); // No cache interaction for non-cacheable operations
    }

    #[test]
    fn test_cache_expiration() {
        let mut cache = Cache::new();
        let value = serde_json::json!({"test": "value"});

        // Put with very short TTL
        cache.put("key1".to_string(), value.clone(), Duration::from_millis(1));

        // Should be available immediately
        assert!(cache.get("key1").is_some());

        // Wait for expiration
        std::thread::sleep(Duration::from_millis(2));

        // Should be expired now
        assert!(cache.get("key1").is_none());
    }
}
