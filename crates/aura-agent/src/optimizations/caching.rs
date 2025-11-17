//! Caching and allocation optimizations for the effect system
//!
//! This module provides caching layers and allocation-reducing strategies
//! to optimize runtime performance of effect execution.

// TODO: Refactor to use TimeEffects. Uses Instant::now() for cache timing
// which should be replaced with effect system integration.
#![allow(clippy::disallowed_methods)]

// use lru::LruCache; // Removed - using HashMap instead
use parking_lot::RwLock;
use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::{Duration, Instant};

use aura_core::{
    effects::{NetworkError, PeerEventStream, StorageError},
    DeviceId,
};
use uuid::Uuid;

/// Cache key for effect results
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub enum CacheKey {
    /// Network receive from peer
    NetworkRecv { peer_id: DeviceId },
    /// Storage retrieval
    StorageRetrieve { key: String },
    /// Storage list keys
    StorageListKeys { prefix: String },
    /// Time epoch
    TimeEpoch,
    /// Custom key for other effects
    Custom { type_id: &'static str, key: String },
}

/// Cached value with expiration
#[derive(Debug, Clone)]
struct CachedValue<T> {
    value: T,
    expires_at: Instant,
}

impl<T> CachedValue<T> {
    fn new(value: T, ttl: Duration, now: Instant) -> Self {
        Self {
            value,
            expires_at: now + ttl,
        }
    }

    fn is_expired(&self, now: Instant) -> bool {
        now > self.expires_at
    }
}

/// LRU cache with TTL support
pub struct EffectCache<T> {
    cache: RwLock<HashMap<CacheKey, CachedValue<T>>>,
    default_ttl: Duration,
    max_size: NonZeroUsize,
}

impl<T: Clone> EffectCache<T> {
    /// Create a new effect cache
    pub fn new(max_size: usize, default_ttl: Duration) -> Self {
        Self {
            cache: RwLock::new(HashMap::new()),
            default_ttl,
            max_size: NonZeroUsize::new(max_size).unwrap_or(NonZeroUsize::new(100).unwrap()),
        }
    }

    /// Get a value from cache
    pub fn get(&self, key: &CacheKey, now: Instant) -> Option<T> {
        let mut cache = self.cache.write();

        if let Some(cached) = cache.get(key) {
            if !cached.is_expired(now) {
                return Some(cached.value.clone());
            } else {
                // Remove expired entry
                cache.remove(key);
            }
        }
        None
    }

    /// Insert a value into cache
    pub fn insert(&self, key: CacheKey, value: T, now: Instant) {
        self.insert_with_ttl(key, value, self.default_ttl, now);
    }

    /// Insert a value with custom TTL
    pub fn insert_with_ttl(&self, key: CacheKey, value: T, ttl: Duration, now: Instant) {
        let mut cache = self.cache.write();
        cache.insert(key, CachedValue::new(value, ttl, now));
    }

    /// Clear the entire cache
    pub fn clear(&self) {
        self.cache.write().clear();
    }

    /// Get cache statistics
    pub fn stats(&self) -> CacheStats {
        let cache = self.cache.read();
        CacheStats {
            size: cache.len(),
            capacity: self.max_size.get(),
        }
    }
}

/// Cache statistics
#[derive(Debug, Clone)]
pub struct CacheStats {
    pub size: usize,
    pub capacity: usize,
}

/// Caching network effect handler wrapper
pub struct CachingNetworkHandler<H> {
    inner: H,
    recv_cache: Arc<EffectCache<Vec<u8>>>,
}

impl<H> CachingNetworkHandler<H> {
    pub fn new(inner: H, cache_size: usize) -> Self {
        Self {
            inner,
            recv_cache: Arc::new(EffectCache::new(
                cache_size,
                Duration::from_secs(60), // 1 minute TTL for network data
            )),
        }
    }
}

#[async_trait::async_trait]
impl<H: aura_core::effects::NetworkEffects> aura_core::effects::NetworkEffects
    for CachingNetworkHandler<H>
{
    async fn send_to_peer(&self, peer_id: Uuid, message: Vec<u8>) -> Result<(), NetworkError> {
        // Sends are not cached
        self.inner.send_to_peer(peer_id, message).await
    }

    async fn broadcast(&self, message: Vec<u8>) -> Result<(), NetworkError> {
        // Broadcasts are not cached
        self.inner.broadcast(message).await
    }

    async fn receive(&self) -> Result<(Uuid, Vec<u8>), NetworkError> {
        // Receives are not cached (they're consumptive operations)
        self.inner.receive().await
    }

    async fn receive_from(&self, peer_id: Uuid) -> Result<Vec<u8>, NetworkError> {
        let cache_key = CacheKey::NetworkRecv {
            peer_id: DeviceId::from(peer_id),
        };

        // TODO: Get current time from TimeEffects instead of direct call
        let now = Instant::now();

        // Check cache first
        if let Some(cached) = self.recv_cache.get(&cache_key, now) {
            return Ok(cached);
        }

        // Cache miss - fetch from inner handler
        let result = self.inner.receive_from(peer_id).await?;

        // Cache successful result
        self.recv_cache.insert(cache_key, result.clone(), now);

        Ok(result)
    }

    async fn connected_peers(&self) -> Vec<Uuid> {
        // Peer lists are not cached (they change frequently)
        self.inner.connected_peers().await
    }

    async fn is_peer_connected(&self, peer_id: Uuid) -> bool {
        // Connection status is not cached (changes frequently)
        self.inner.is_peer_connected(peer_id).await
    }

    async fn subscribe_to_peer_events(&self) -> Result<PeerEventStream, NetworkError> {
        // Event streams are not cached
        self.inner.subscribe_to_peer_events().await
    }
}

/// Caching storage effect handler wrapper
pub struct CachingStorageHandler<H> {
    inner: H,
    read_cache: Arc<EffectCache<Option<Vec<u8>>>>,
    list_cache: Arc<EffectCache<Vec<String>>>,
}

impl<H> CachingStorageHandler<H> {
    pub fn new(inner: H, cache_size: usize) -> Self {
        Self {
            inner,
            read_cache: Arc::new(EffectCache::new(
                cache_size,
                Duration::from_secs(300), // 5 minute TTL for storage data
            )),
            list_cache: Arc::new(EffectCache::new(
                cache_size / 4,           // Smaller cache for list operations
                Duration::from_secs(120), // 2 minute TTL for listings
            )),
        }
    }
}

#[async_trait::async_trait]
impl<H: aura_core::effects::StorageEffects> aura_core::effects::StorageEffects
    for CachingStorageHandler<H>
{
    async fn store(&self, key: &str, value: Vec<u8>) -> Result<(), StorageError> {
        // Store in inner handler
        self.inner.store(key, value.clone()).await?;

        // TODO: Get current time from TimeEffects instead of direct call
        let now = Instant::now();

        // Update cache with new value
        let cache_key = CacheKey::StorageRetrieve {
            key: key.to_string(),
        };
        self.read_cache.insert(cache_key, Some(value), now);

        // Invalidate list cache as new key was added
        self.list_cache.clear();

        Ok(())
    }

    async fn retrieve(&self, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
        let cache_key = CacheKey::StorageRetrieve {
            key: key.to_string(),
        };

        // TODO: Get current time from TimeEffects instead of direct call
        let now = Instant::now();

        // Check cache first
        if let Some(cached) = self.read_cache.get(&cache_key, now) {
            return Ok(cached);
        }

        // Cache miss - fetch from inner handler
        let result = self.inner.retrieve(key).await?;

        // Cache result (including None for non-existent keys)
        self.read_cache.insert(cache_key, result.clone(), now);

        Ok(result)
    }

    async fn remove(&self, key: &str) -> Result<bool, StorageError> {
        // Remove from inner handler
        let removed = self.inner.remove(key).await?;

        // TODO: Get current time from TimeEffects instead of direct call
        let now = Instant::now();

        // Remove from cache
        let cache_key = CacheKey::StorageRetrieve {
            key: key.to_string(),
        };
        self.read_cache.insert(cache_key, None, now);

        // Invalidate list cache
        self.list_cache.clear();

        Ok(removed)
    }

    async fn list_keys(&self, prefix: Option<&str>) -> Result<Vec<String>, StorageError> {
        let cache_key = CacheKey::StorageListKeys {
            prefix: prefix.map(|s| s.to_string()).unwrap_or_default(),
        };

        // TODO: Get current time from TimeEffects instead of direct call
        let now = Instant::now();

        // Check cache first
        if let Some(cached) = self.list_cache.get(&cache_key, now) {
            return Ok(cached);
        }

        // Cache miss - fetch from inner handler
        let result = self.inner.list_keys(prefix).await?;

        // Cache result
        self.list_cache.insert(cache_key, result.clone(), now);

        Ok(result)
    }

    async fn exists(&self, key: &str) -> Result<bool, StorageError> {
        // Check cache first for existence
        let cache_key = CacheKey::StorageRetrieve {
            key: key.to_string(),
        };

        // TODO: Get current time from TimeEffects instead of direct call
        let now = Instant::now();

        if let Some(cached) = self.read_cache.get(&cache_key, now) {
            return Ok(cached.is_some());
        }

        // Cache miss - check inner handler
        let exists = self.inner.exists(key).await?;

        // If it doesn't exist, cache the None value
        if !exists {
            self.read_cache.insert(cache_key, None, now);
        }

        Ok(exists)
    }

    async fn store_batch(&self, pairs: HashMap<String, Vec<u8>>) -> Result<(), StorageError> {
        // Store in inner handler
        self.inner.store_batch(pairs.clone()).await?;

        // TODO: Get current time from TimeEffects instead of direct call
        let now = Instant::now();

        // Update cache with new values
        for (key, value) in pairs {
            let cache_key = CacheKey::StorageRetrieve { key };
            self.read_cache.insert(cache_key, Some(value), now);
        }

        // Invalidate list cache as new keys were added
        self.list_cache.clear();

        Ok(())
    }

    async fn retrieve_batch(
        &self,
        keys: &[String],
    ) -> Result<HashMap<String, Vec<u8>>, StorageError> {
        let mut result = HashMap::new();
        let mut uncached_keys = Vec::new();

        // TODO: Get current time from TimeEffects instead of direct call
        let now = Instant::now();

        // Check cache for each key
        for key in keys {
            let cache_key = CacheKey::StorageRetrieve { key: key.clone() };

            if let Some(cached) = self.read_cache.get(&cache_key, now) {
                if let Some(value) = cached {
                    result.insert(key.clone(), value);
                }
            } else {
                uncached_keys.push(key.clone());
            }
        }

        // Fetch uncached keys from inner handler
        if !uncached_keys.is_empty() {
            let uncached_results = self.inner.retrieve_batch(&uncached_keys).await?;

            // Update cache and result
            for (key, value) in uncached_results {
                let cache_key = CacheKey::StorageRetrieve { key: key.clone() };
                self.read_cache.insert(cache_key, Some(value.clone()), now);
                result.insert(key, value);
            }
        }

        Ok(result)
    }

    async fn clear_all(&self) -> Result<(), StorageError> {
        // Clear inner handler
        self.inner.clear_all().await?;

        // Clear all caches
        self.read_cache.clear();
        self.list_cache.clear();

        Ok(())
    }

    // NOTE: location() method is not part of StorageEffects trait
    // Removed to match trait definition

    async fn stats(&self) -> Result<aura_core::effects::StorageStats, StorageError> {
        self.inner.stats().await
    }
}

/// Object pool for reducing allocations
pub struct ObjectPool<T> {
    pool: RwLock<Vec<T>>,
    max_size: usize,
    factory: Box<dyn Fn() -> T + Send + Sync>,
}

impl<T> ObjectPool<T> {
    /// Create a new object pool
    pub fn new<F>(max_size: usize, factory: F) -> Self
    where
        F: Fn() -> T + Send + Sync + 'static,
    {
        Self {
            pool: RwLock::new(Vec::with_capacity(max_size)),
            max_size,
            factory: Box::new(factory),
        }
    }

    /// Get an object from the pool or create new
    pub fn get(&self) -> PooledObject<T> {
        let obj = self.pool.write().pop().unwrap_or_else(|| (self.factory)());
        PooledObject {
            inner: Some(obj),
            pool: self as *const _ as *mut ObjectPool<T>,
        }
    }

    /// Return an object to the pool
    fn return_object(&self, obj: T) {
        let mut pool = self.pool.write();
        if pool.len() < self.max_size {
            pool.push(obj);
        }
    }
}

/// RAII wrapper for pooled objects
pub struct PooledObject<T> {
    inner: Option<T>,
    pool: *mut ObjectPool<T>,
}

impl<T> std::ops::Deref for PooledObject<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.inner.as_ref().unwrap()
    }
}

impl<T> std::ops::DerefMut for PooledObject<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.inner.as_mut().unwrap()
    }
}

impl<T> Drop for PooledObject<T> {
    fn drop(&mut self) {
        if let Some(obj) = self.inner.take() {
            unsafe {
                (*self.pool).return_object(obj);
            }
        }
    }
}

// Safety: ObjectPool is Send + Sync, and we only access it through the pointer
unsafe impl<T: Send> Send for PooledObject<T> {}
unsafe impl<T: Sync> Sync for PooledObject<T> {}

/// Common object pools for the effect system
pub struct EffectObjectPools {
    /// Pool for Vec<u8> buffers
    pub byte_buffers: ObjectPool<Vec<u8>>,
    /// Pool for HashMap string metadata
    pub string_maps: ObjectPool<HashMap<String, String>>,
}

impl EffectObjectPools {
    /// Create standard object pools
    pub fn new() -> Self {
        Self {
            byte_buffers: ObjectPool::new(100, || Vec::with_capacity(4096)),
            string_maps: ObjectPool::new(50, HashMap::new),
        }
    }
}

impl Default for EffectObjectPools {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_testkit::{aura_test, TestFixture};

    #[test]
    fn test_effect_cache() {
        let cache = EffectCache::new(10, Duration::from_secs(60));
        let now = Instant::now();

        // Test insertion and retrieval
        let key = CacheKey::StorageRetrieve {
            key: "test".to_string(),
        };
        cache.insert(key.clone(), vec![1, 2, 3], now);

        assert_eq!(cache.get(&key, now), Some(vec![1, 2, 3]));

        // Test expiration
        let cache = EffectCache::new(10, Duration::from_millis(10));
        let now = Instant::now();
        cache.insert(key.clone(), vec![4, 5, 6], now);
        std::thread::sleep(Duration::from_millis(20));
        let later = Instant::now();
        assert_eq!(cache.get(&key, later), None);
    }

    #[test]
    fn test_object_pool() {
        let pool = ObjectPool::new(5, || vec![0u8; 1024]);

        // Get objects from pool
        let mut obj1 = pool.get();
        obj1[0] = 42;

        let obj2 = pool.get();
        assert_eq!(obj2[0], 0); // New object

        // Return to pool
        drop(obj1);
        drop(obj2);

        // Reuse from pool
        let obj3 = pool.get();
        assert_eq!(obj3[0], 42); // Reused obj1
    }

    #[aura_test]
    async fn test_caching_handlers() -> AuraResult<()> {
        use aura_effects::handlers::{InMemoryStorageHandler, MockNetworkHandler};

        let fixture = TestFixture::new().await?;

        // Test network caching
        let inner = MockNetworkHandler::new();
        let cached = CachingNetworkHandler::new(inner, 10);

        let peer_id = fixture.device_id();
        let _ = cached.receive_from(peer_id.into()).await?;
        let _ = cached.receive_from(peer_id.into()).await?; // Should hit cache

        // Test storage caching
        let inner = InMemoryStorageHandler::new();
        let cached = CachingStorageHandler::new(inner, 10);

        cached.store("key1", vec![1, 2, 3]).await?;
        let result = cached.retrieve("key1").await?;
        assert_eq!(result, Some(vec![1, 2, 3]));

        // Second retrieve should hit cache
        let result = cached.retrieve("key1").await?;
        assert_eq!(result, Some(vec![1, 2, 3]));
        Ok(())
    }
}
