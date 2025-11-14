//! Examples demonstrating performance optimization techniques
//!
//! This example shows how to use the various performance optimizations
//! available in the effect system.

use std::time::Instant;
use std::sync::Arc;

use aura_protocol::effects::{
    AuraEffectSystem, EffectSystemConfig,
    ParallelInitBuilder, InitializationMetrics,
    CachingNetworkHandler, CachingStorageHandler, EffectCache, CacheKey,
    allocations::{intern, StringInterner, BufferPool, Arena, SmallVec, BUFFER_POOL},
    LazyEffectSystem, HandlerPool,
    NetworkEffects, StorageEffects,
};
use aura_core::DeviceId;
use aura_effects::handlers::{MockNetworkHandler, InMemoryStorageHandler};

/// Example 1: Parallel initialization
async fn example_parallel_initialization() {
    println!("\n=== Example 1: Parallel Initialization ===");
    
    let device_id = DeviceId::new();
    let config = EffectSystemConfig::for_testing(device_id);
    
    // Standard initialization
    let start = Instant::now();
    let _system1 = AuraEffectSystem::new().await;
    let standard_time = start.elapsed();
    
    // Parallel initialization with metrics
    let builder = ParallelInitBuilder::new(config)
        .with_metrics();
    
    let (system2, metrics) = builder.build().await.unwrap();
    let metrics = metrics.unwrap();
    
    println!("Standard init: {:?}", standard_time);
    println!("Parallel init: {:?}", metrics.total_duration);
    println!("  - Handler init: {:?}", metrics.handler_init_duration);
    println!("  - Service init: {:?}", metrics.service_init_duration);
    println!("  - Speedup: {:.2}x", metrics.parallel_speedup);
    
    // Verify functionality
    assert_eq!(system2.current_epoch().await, 1);
}

/// Example 2: Lazy initialization
async fn example_lazy_initialization() {
    println!("\n=== Example 2: Lazy Initialization ===");
    
    let config = EffectSystemConfig::for_testing(DeviceId::new());
    let lazy_system = LazyEffectSystem::new(config);
    
    println!("LazyEffectSystem created (not initialized yet)");
    
    // First access triggers initialization
    let start = Instant::now();
    let system = lazy_system.get().await.unwrap();
    let init_time = start.elapsed();
    
    println!("First access (initialization): {:?}", init_time);
    
    // Second access is instant
    let start = Instant::now();
    let system2 = lazy_system.get().await.unwrap();
    let access_time = start.elapsed();
    
    println!("Second access (cached): {:?}", access_time);
    
    // Verify same instance
    assert!(Arc::ptr_eq(system, system2));
}

/// Example 3: Effect caching
async fn example_effect_caching() {
    println!("\n=== Example 3: Effect Caching ===");
    
    // Create cached storage handler
    let base = InMemoryStorageHandler::new();
    let cached = CachingStorageHandler::new(base, 100);
    
    // Populate some data
    for i in 0..10 {
        cached.store(&format!("key_{}", i), vec![i as u8; 100], false).await.unwrap();
    }
    
    // First retrieval (cache miss)
    let start = Instant::now();
    let data1 = cached.retrieve("key_5").await.unwrap();
    let miss_time = start.elapsed();
    
    // Second retrieval (cache hit)
    let start = Instant::now();
    let data2 = cached.retrieve("key_5").await.unwrap();
    let hit_time = start.elapsed();
    
    println!("Cache miss: {:?}", miss_time);
    println!("Cache hit: {:?} (speedup: {:.2}x)", 
        hit_time, 
        miss_time.as_nanos() as f64 / hit_time.as_nanos() as f64
    );
    
    assert_eq!(data1, data2);
    
    // Direct cache usage
    let cache: EffectCache<String> = EffectCache::new(50, std::time::Duration::from_secs(60));
    
    let key = CacheKey::Custom { type_id: "example", key: "test".to_string() };
    cache.insert(key.clone(), "cached value".to_string());
    
    assert_eq!(cache.get(&key), Some("cached value".to_string()));
}

/// Example 4: String interning
fn example_string_interning() {
    println!("\n=== Example 4: String Interning ===");
    
    let interner = StringInterner::new();
    
    // Measure allocation overhead
    let start = Instant::now();
    let mut strings = Vec::new();
    for i in 0..1000 {
        strings.push(format!("string_{}", i % 10));
    }
    let alloc_time = start.elapsed();
    
    // Measure interning overhead
    let start = Instant::now();
    let mut atoms = Vec::new();
    for i in 0..1000 {
        let s = format!("string_{}", i % 10);
        atoms.push(interner.intern(&s));
    }
    let intern_time = start.elapsed();
    
    println!("Regular allocation: {:?}", alloc_time);
    println!("With interning: {:?}", intern_time);
    println!("Interned strings: {}", interner.stats().interned_count);
    
    // Global interner
    let atom1 = intern("global_string");
    let atom2 = intern("global_string");
    assert_eq!(atom1, atom2); // Same atom
}

/// Example 5: Buffer pools
async fn example_buffer_pools() {
    println!("\n=== Example 5: Buffer Pools ===");
    
    let pool = BufferPool::new();
    
    // Pre-warm the pool
    for _ in 0..10 {
        let buf = pool.get_buffer(4096);
        pool.return_buffer(buf);
    }
    
    // Measure allocation without pool
    let start = Instant::now();
    for _ in 0..1000 {
        let _buf = Vec::<u8>::with_capacity(4096);
    }
    let no_pool_time = start.elapsed();
    
    // Measure with pool
    let start = Instant::now();
    for _ in 0..1000 {
        let buf = pool.get_buffer(4096);
        pool.return_buffer(buf);
    }
    let with_pool_time = start.elapsed();
    
    println!("Without pool: {:?}", no_pool_time);
    println!("With pool: {:?} (speedup: {:.2}x)", 
        with_pool_time,
        no_pool_time.as_nanos() as f64 / with_pool_time.as_nanos() as f64
    );
    
    // Global buffer pool
    let buf = BUFFER_POOL.get_buffer(1024);
    println!("Got buffer from global pool: {} bytes", buf.capacity());
    BUFFER_POOL.return_buffer(buf);
}

/// Example 6: Arena allocation
fn example_arena_allocation() {
    println!("\n=== Example 6: Arena Allocation ===");
    
    let arena = Arena::new(1024 * 1024); // 1MB chunks
    
    // Regular string allocation
    let start = Instant::now();
    let mut strings = Vec::new();
    for i in 0..10000 {
        strings.push(format!("string_{}", i));
    }
    let vec_time = start.elapsed();
    
    // Arena allocation
    let start = Instant::now();
    let mut arena_strings = Vec::new();
    for i in 0..10000 {
        let s = arena.alloc_str(&format!("string_{}", i));
        arena_strings.push(s);
    }
    let arena_time = start.elapsed();
    
    println!("Vec allocation: {:?}", vec_time);
    println!("Arena allocation: {:?} (speedup: {:.2}x)",
        arena_time,
        vec_time.as_nanos() as f64 / arena_time.as_nanos() as f64
    );
    
    let stats = arena.stats();
    println!("Arena stats: {} chunks, {} bytes used", stats.chunks, stats.used);
}

/// Example 7: SmallVec optimization
fn example_small_vec() {
    println!("\n=== Example 7: SmallVec Optimization ===");
    
    // Compare SmallVec vs Vec for small collections
    let start = Instant::now();
    let mut vecs = Vec::new();
    for i in 0..10000 {
        let mut v = Vec::new();
        v.push(i);
        v.push(i + 1);
        vecs.push(v);
    }
    let vec_time = start.elapsed();
    
    let start = Instant::now();
    let mut small_vecs = Vec::new();
    for i in 0..10000 {
        let mut v = SmallVec::new();
        v.push(i);
        v.push(i + 1);
        small_vecs.push(v);
    }
    let small_vec_time = start.elapsed();
    
    println!("Vec time: {:?}", vec_time);
    println!("SmallVec time: {:?} (speedup: {:.2}x)",
        small_vec_time,
        vec_time.as_nanos() as f64 / small_vec_time.as_nanos() as f64
    );
    
    // Memory comparison (approximate)
    let vec_size = std::mem::size_of::<Vec<i32>>() * vecs.len();
    let small_vec_size = std::mem::size_of::<SmallVec<i32>>() * small_vecs.len();
    
    println!("Vec memory: ~{} bytes", vec_size);
    println!("SmallVec memory: ~{} bytes (savings: {}%)",
        small_vec_size,
        ((vec_size - small_vec_size) * 100) / vec_size
    );
}

/// Example 8: Handler pooling
async fn example_handler_pooling() {
    println!("\n=== Example 8: Handler Pooling ===");
    
    let mut pool = HandlerPool::new(20);
    
    // Pre-warm the pool
    pool.warm_up(10).await;
    
    // Measure without pooling
    let start = Instant::now();
    for _ in 0..100 {
        let _handler = Arc::new(MockNetworkHandler::new());
    }
    let no_pool_time = start.elapsed();
    
    // Measure with pooling
    let start = Instant::now();
    for _ in 0..100 {
        let handler = pool.get_network_handler();
        pool.return_network_handler(handler);
    }
    let with_pool_time = start.elapsed();
    
    println!("Without pooling: {:?}", no_pool_time);
    println!("With pooling: {:?} (speedup: {:.2}x)",
        with_pool_time,
        no_pool_time.as_nanos() as f64 / with_pool_time.as_nanos() as f64
    );
}

/// Example 9: Combined optimizations
async fn example_combined_optimizations() {
    println!("\n=== Example 9: Combined Optimizations ===");
    
    let device_id = DeviceId::new();
    let config = EffectSystemConfig::for_testing(device_id);
    
    // Create optimized effect system
    let start = Instant::now();
    
    // 1. Parallel initialization
    let builder = ParallelInitBuilder::new(config);
    let (system, _) = builder.build().await.unwrap();
    
    // 2. Wrap handlers with caching
    let base_storage = InMemoryStorageHandler::new();
    let cached_storage = Arc::new(CachingStorageHandler::new(base_storage, 1000));
    
    // 3. Use buffer pool for operations
    let buffer_pool = BufferPool::new();
    
    // 4. Use string interning for keys
    let interner = StringInterner::new();
    
    let init_time = start.elapsed();
    println!("Optimized system initialization: {:?}", init_time);
    
    // Run operations with all optimizations
    let start = Instant::now();
    
    for i in 0..1000 {
        // Use interned strings for keys
        let key = interner.intern(&format!("key_{}", i % 100));
        
        // Get buffer from pool
        let mut buffer = buffer_pool.get_buffer(256);
        buffer.extend_from_slice(&[i as u8; 256]);
        
        // Store with caching
        cached_storage.store(key.as_ref(), buffer.clone(), false).await.unwrap();
        
        // Return buffer to pool
        buffer_pool.return_buffer(buffer);
    }
    
    let ops_time = start.elapsed();
    let ops_per_sec = 1000.0 / ops_time.as_secs_f64();
    
    println!("1000 operations completed in: {:?}", ops_time);
    println!("Operations per second: {:.0}", ops_per_sec);
}

#[tokio::main]
async fn main() {
    println!("Performance Optimization Examples\n");
    
    example_parallel_initialization().await;
    example_lazy_initialization().await;
    example_effect_caching().await;
    example_string_interning();
    example_buffer_pools().await;
    example_arena_allocation();
    example_small_vec();
    example_handler_pooling().await;
    example_combined_optimizations().await;
    
    println!("\nAll examples completed!");
}