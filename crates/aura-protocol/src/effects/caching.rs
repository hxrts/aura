//! Caching and allocation optimizations for the effect system
//!
//! This module provides caching layers and allocation-reducing strategies
//! to optimize runtime performance of effect execution.

use std::sync::Arc;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::time::{Duration, Instant};
use parking_lot::RwLock;
use lru::LruCache;
use std::num::NonZeroUsize;

use aura_core::{
    AuraResult, AuraError, DeviceId,
    effects::{NetworkError, StorageError},
};

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
    fn new(value: T, ttl: Duration) -> Self {
        Self {
            value,
            expires_at: Instant::now() + ttl,
        }
    }

    fn is_expired(&self) -> bool {
        Instant::now() > self.expires_at
    }
}

/// LRU cache with TTL support
pub struct EffectCache<T> {
    cache: RwLock<LruCache<CacheKey, CachedValue<T>>>,
    default_ttl: Duration,
    max_size: NonZeroUsize,
}

impl<T: Clone> EffectCache<T> {
    /// Create a new effect cache
    pub fn new(max_size: usize, default_ttl: Duration) -> Self {
        Self {
            cache: RwLock::new(LruCache::new(
                NonZeroUsize::new(max_size).unwrap_or(NonZeroUsize::new(100).unwrap())
            )),
            default_ttl,
            max_size: NonZeroUsize::new(max_size).unwrap_or(NonZeroUsize::new(100).unwrap()),
        }
    }

    /// Get a value from cache
    pub fn get(&self, key: &CacheKey) -> Option<T> {
        let mut cache = self.cache.write();
        
        if let Some(cached) = cache.get(key) {
            if !cached.is_expired() {
                return Some(cached.value.clone());
            } else {
                // Remove expired entry
                cache.pop(key);
            }
        }
        None
    }

    /// Insert a value into cache
    pub fn insert(&self, key: CacheKey, value: T) {
        self.insert_with_ttl(key, value, self.default_ttl);
    }

    /// Insert a value with custom TTL
    pub fn insert_with_ttl(&self, key: CacheKey, value: T, ttl: Duration) {
        let mut cache = self.cache.write();
        cache.put(key, CachedValue::new(value, ttl));
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
impl<H: aura_core::effects::NetworkEffects> aura_core::effects::NetworkEffects for CachingNetworkHandler<H> {
    async fn send_to_peer(
        &self,
        peer_id: DeviceId,
        message: Vec<u8>,
    ) -> Result<(), NetworkError> {
        // Sends are not cached
        self.inner.send_to_peer(peer_id, message).await
    }

    async fn recv_from_peer(
        &self,
        peer_id: DeviceId,
    ) -> Result<Vec<u8>, NetworkError> {
        let cache_key = CacheKey::NetworkRecv { peer_id };
        
        // Check cache first
        if let Some(cached) = self.recv_cache.get(&cache_key) {
            return Ok(cached);
        }
        
        // Cache miss - fetch from inner handler
        let result = self.inner.recv_from_peer(peer_id).await?;
        
        // Cache successful result
        self.recv_cache.insert(cache_key, result.clone());
        
        Ok(result)
    }

    async fn broadcast(&self, message: Vec<u8>) -> Result<(), NetworkError> {
        // Broadcasts are not cached
        self.inner.broadcast(message).await
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
                cache_size / 4, // Smaller cache for list operations
                Duration::from_secs(120), // 2 minute TTL for listings
            )),
        }
    }
}

#[async_trait::async_trait]
impl<H: aura_core::effects::StorageEffects> aura_core::effects::StorageEffects for CachingStorageHandler<H> {
    async fn store(
        &self,
        key: &str,
        value: Vec<u8>,
        encrypted: bool,
    ) -> Result<(), StorageError> {
        // Store in inner handler
        self.inner.store(key, value.clone(), encrypted).await?;
        
        // Update cache with new value
        let cache_key = CacheKey::StorageRetrieve { key: key.to_string() };
        self.read_cache.insert(cache_key, Some(value));
        
        // Invalidate list cache as new key was added
        self.list_cache.clear();
        
        Ok(())
    }

    async fn retrieve(&self, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
        let cache_key = CacheKey::StorageRetrieve { key: key.to_string() };
        
        // Check cache first
        if let Some(cached) = self.read_cache.get(&cache_key) {
            return Ok(cached);
        }
        
        // Cache miss - fetch from inner handler
        let result = self.inner.retrieve(key).await?;
        
        // Cache result (including None for non-existent keys)
        self.read_cache.insert(cache_key, result.clone());
        
        Ok(result)
    }

    async fn delete(&self, key: &str) -> Result<(), StorageError> {
        // Delete from inner handler
        self.inner.delete(key).await?;
        
        // Remove from cache
        let cache_key = CacheKey::StorageRetrieve { key: key.to_string() };
        self.read_cache.insert(cache_key, None);
        
        // Invalidate list cache
        self.list_cache.clear();
        
        Ok(())
    }

    async fn list_keys(&self, prefix: &str) -> Result<Vec<String>, StorageError> {
        let cache_key = CacheKey::StorageListKeys { prefix: prefix.to_string() };
        
        // Check cache first
        if let Some(cached) = self.list_cache.get(&cache_key) {
            return Ok(cached);
        }
        
        // Cache miss - fetch from inner handler
        let result = self.inner.list_keys(prefix).await?;
        
        // Cache result
        self.list_cache.insert(cache_key, result.clone());
        
        Ok(result)
    }

    fn location(&self) -> aura_core::effects::StorageLocation {
        self.inner.location()
    }

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

    #[test]
    fn test_effect_cache() {
        let cache = EffectCache::new(10, Duration::from_secs(60));
        
        // Test insertion and retrieval
        let key = CacheKey::StorageRetrieve { key: "test".to_string() };
        cache.insert(key.clone(), vec![1, 2, 3]);
        
        assert_eq!(cache.get(&key), Some(vec![1, 2, 3]));
        
        // Test expiration
        let cache = EffectCache::new(10, Duration::from_millis(10));
        cache.insert(key.clone(), vec![4, 5, 6]);
        std::thread::sleep(Duration::from_millis(20));
        assert_eq!(cache.get(&key), None);
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

    #[tokio::test]
    async fn test_caching_handlers() {
        use aura_effects::handlers::{MockNetworkHandler, InMemoryStorageHandler};
        
        // Test network caching
        let inner = MockNetworkHandler::new();
        let cached = CachingNetworkHandler::new(inner, 10);
        
        let peer_id = DeviceId::new();
        let _ = cached.recv_from_peer(peer_id).await.unwrap();
        let _ = cached.recv_from_peer(peer_id).await.unwrap(); // Should hit cache
        
        // Test storage caching
        let inner = InMemoryStorageHandler::new();
        let cached = CachingStorageHandler::new(inner, 10);
        
        cached.store("key1", vec![1, 2, 3], false).await.unwrap();
        let result = cached.retrieve("key1").await.unwrap();
        assert_eq!(result, Some(vec![1, 2, 3]));
        
        // Second retrieve should hit cache
        let result = cached.retrieve("key1").await.unwrap();
        assert_eq!(result, Some(vec![1, 2, 3]));
    }
}