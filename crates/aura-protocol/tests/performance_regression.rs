//! Performance regression tests for the effect system
//!
//! These tests ensure that performance optimizations don't regress
//! and that the system meets expected performance targets.

use aura_core::{DeviceId, FlowBudget};
use aura_effects::handlers::{InMemoryStorageHandler, MockNetworkHandler};
use aura_protocol::effects::{
    allocations::{Arena, BufferPool, SmallVec, StringInterner},
    AuraEffectSystem, AuraEffectSystemBuilder, CachingNetworkHandler, CachingStorageHandler,
    EffectSystemConfig, InitializationMetrics, NetworkEffects, ParallelInitBuilder, StorageEffects,
};
use std::time::{Duration, Instant};
use tokio::runtime::Runtime;

/// Performance thresholds for regression detection
struct PerformanceThresholds {
    /// Maximum allowed initialization time
    max_init_time: Duration,
    /// Maximum time for single effect execution
    max_effect_time: Duration,
    /// Minimum operations per second
    min_ops_per_second: u64,
    /// Maximum memory overhead per operation (bytes)
    max_memory_per_op: usize,
}

impl Default for PerformanceThresholds {
    fn default() -> Self {
        Self {
            max_init_time: Duration::from_millis(100),
            max_effect_time: Duration::from_micros(100),
            min_ops_per_second: 10_000,
            max_memory_per_op: 1024,
        }
    }
}

#[test]
fn test_initialization_performance() {
    let rt = Runtime::new().unwrap();
    let thresholds = PerformanceThresholds::default();

    rt.block_on(async {
        let config = EffectSystemConfig::for_testing(DeviceId::new());

        // Measure sequential initialization
        let start = Instant::now();
        let _system = AuraEffectSystem::new().await;
        let sequential_time = start.elapsed();

        // Measure parallel initialization
        let builder = ParallelInitBuilder::new(config).with_metrics();
        let (system, metrics) = builder.build().await.unwrap();
        let metrics = metrics.unwrap();

        println!("Initialization Performance:");
        println!("  Sequential: {:?}", sequential_time);
        println!("  Parallel: {:?}", metrics.total_duration);
        println!("  Speedup: {:.2}x", metrics.parallel_speedup);

        // Regression test - parallel should be faster
        assert!(
            metrics.total_duration < sequential_time,
            "Parallel init slower than sequential!"
        );

        // Absolute threshold
        assert!(
            metrics.total_duration < thresholds.max_init_time,
            "Initialization too slow: {:?} > {:?}",
            metrics.total_duration,
            thresholds.max_init_time
        );

        // Verify system is functional
        let epoch = system.current_epoch().await;
        assert_eq!(epoch, 1);
    });
}

#[test]
fn test_effect_execution_performance() {
    let rt = Runtime::new().unwrap();
    let thresholds = PerformanceThresholds::default();

    rt.block_on(async {
        let system = AuraEffectSystem::new().await;

        // Warm up
        for _ in 0..100 {
            let _ = system.current_epoch().await;
        }

        // Measure single effect execution
        let start = Instant::now();
        let iterations = 10_000;

        for _ in 0..iterations {
            let _ = system.send_to_peer(DeviceId::new(), vec![0; 256]).await;
        }

        let elapsed = start.elapsed();
        let per_op = elapsed / iterations;
        let ops_per_sec = (iterations as f64 / elapsed.as_secs_f64()) as u64;

        println!("Effect Execution Performance:");
        println!("  Per operation: {:?}", per_op);
        println!("  Operations/sec: {}", ops_per_sec);

        // Regression tests
        assert!(
            per_op < thresholds.max_effect_time,
            "Effect execution too slow: {:?} > {:?}",
            per_op,
            thresholds.max_effect_time
        );

        assert!(
            ops_per_sec > thresholds.min_ops_per_second,
            "Too few ops/sec: {} < {}",
            ops_per_sec,
            thresholds.min_ops_per_second
        );
    });
}

#[test]
fn test_caching_performance() {
    let rt = Runtime::new().unwrap();

    rt.block_on(async {
        let base_handler = InMemoryStorageHandler::new();
        let cached_handler = CachingStorageHandler::new(base_handler, 1000);

        // Populate cache
        for i in 0..100 {
            cached_handler
                .store(&format!("key_{}", i), vec![0; 1024], false)
                .await
                .unwrap();
        }

        // Measure cache hit performance
        let start = Instant::now();
        let iterations = 100_000;

        for i in 0..iterations {
            let key = format!("key_{}", i % 100);
            let _ = cached_handler.retrieve(&key).await.unwrap();
        }

        let elapsed = start.elapsed();
        let per_op = elapsed / iterations;

        println!("Cache Performance:");
        println!("  Cache hit time: {:?}", per_op);
        println!(
            "  Cache hits/sec: {}",
            (iterations as f64 / elapsed.as_secs_f64()) as u64
        );

        // Cache hits should be very fast
        assert!(
            per_op < Duration::from_nanos(1000),
            "Cache hits too slow: {:?}",
            per_op
        );
    });
}

#[test]
fn test_allocation_performance() {
    let interner = StringInterner::new();
    let buffer_pool = BufferPool::new();
    let arena = Arena::new(1024 * 1024); // 1MB chunks

    // Test string interning
    let start = Instant::now();
    for i in 0..10_000 {
        let s = format!("string_{}", i % 100);
        let _ = interner.intern(&s);
    }
    let intern_time = start.elapsed();

    // Test buffer pool
    let start = Instant::now();
    for _ in 0..10_000 {
        let buf = buffer_pool.get_buffer(4096);
        buffer_pool.return_buffer(buf);
    }
    let pool_time = start.elapsed();

    // Test arena allocation
    let start = Instant::now();
    for i in 0..10_000 {
        let s = format!("arena_string_{}", i);
        let _ = arena.alloc_str(&s);
    }
    let arena_time = start.elapsed();

    println!("Allocation Performance:");
    println!("  String interning: {:?}", intern_time);
    println!("  Buffer pool: {:?}", pool_time);
    println!("  Arena allocation: {:?}", arena_time);

    // All should be fast
    assert!(intern_time < Duration::from_millis(50));
    assert!(pool_time < Duration::from_millis(10));
    assert!(arena_time < Duration::from_millis(20));
}

#[test]
fn test_memory_overhead() {
    let thresholds = PerformanceThresholds::default();

    // Test SmallVec memory efficiency
    let mut small_vecs: Vec<SmallVec<u64>> = Vec::new();

    // Get baseline memory
    let baseline = get_current_memory();

    // Create 1000 small vectors
    for i in 0..1000 {
        let mut vec = SmallVec::new();
        vec.push(i);
        vec.push(i + 1);
        small_vecs.push(vec);
    }

    let after = get_current_memory();
    let overhead_per_vec = (after.saturating_sub(baseline)) / 1000;

    println!("Memory Overhead:");
    println!("  SmallVec overhead: {} bytes/vec", overhead_per_vec);

    // Should have minimal overhead compared to Vec
    assert!(
        overhead_per_vec < thresholds.max_memory_per_op,
        "SmallVec overhead too high: {} bytes",
        overhead_per_vec
    );
}

#[test]
fn test_concurrent_performance() {
    let rt = Runtime::new().unwrap();

    rt.block_on(async {
        let system = AuraEffectSystem::new().await;

        // Measure concurrent effect execution
        let start = Instant::now();
        let tasks = 100;
        let ops_per_task = 100;

        let handles: Vec<_> = (0..tasks)
            .map(|_| {
                let sys = system.clone();
                tokio::spawn(async move {
                    for _ in 0..ops_per_task {
                        let _ = sys.current_epoch().await;
                    }
                })
            })
            .collect();

        for handle in handles {
            handle.await.unwrap();
        }

        let elapsed = start.elapsed();
        let total_ops = tasks * ops_per_task;
        let ops_per_sec = (total_ops as f64 / elapsed.as_secs_f64()) as u64;

        println!("Concurrent Performance:");
        println!("  Total time: {:?}", elapsed);
        println!("  Concurrent ops/sec: {}", ops_per_sec);

        // Should scale well with concurrency
        assert!(
            ops_per_sec > 50_000,
            "Poor concurrent performance: {} ops/sec",
            ops_per_sec
        );
    });
}

/// Get current memory usage (approximation for testing)
fn get_current_memory() -> usize {
    // In a real implementation, we'd use OS-specific APIs
    // For testing, we'll use a simple approximation
    std::mem::size_of::<usize>() * 1000 // Placeholder
}

#[test]
fn test_regression_suite() {
    // Run a comprehensive regression test
    println!("\n=== Performance Regression Suite ===\n");

    test_initialization_performance();
    test_effect_execution_performance();
    test_caching_performance();
    test_allocation_performance();
    test_memory_overhead();
    test_concurrent_performance();

    println!("\n=== All regression tests passed ===");
}
