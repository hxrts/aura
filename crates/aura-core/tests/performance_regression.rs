//! Performance regression tests for unified time system
//!
//! These tests establish performance baselines and catch regressions.
//! They run faster than full benchmarks but provide regression detection.

use aura_core::identifiers::DeviceId;
use aura_core::time::{OrderingPolicy, PhysicalTime, TimeStamp, VectorClock};
use std::time::Instant;

/// Performance test thresholds (in nanoseconds)  
/// Note: These are set for debug mode and are more lenient than release benchmarks
const TIME_ACCESS_THRESHOLD_NS: u64 = 200;
const TIMESTAMP_COMPARE_THRESHOLD_NS: u64 = 100;
const VECTORCLOCK_SINGLE_THRESHOLD_NS: u64 = 100; // More lenient for debug builds
const VECTORCLOCK_MULTI_THRESHOLD_NS: u64 = 5000; // Much more lenient for debug builds

/// Quick performance test for time access operations
#[test]
fn test_time_access_performance() {
    let physical_time = PhysicalTime {
        ts_ms: 1000000,
        uncertainty: None,
    };
    let timestamp = TimeStamp::PhysicalClock(physical_time);

    let start = Instant::now();
    for _ in 0..1000 {
        let _ = std::hint::black_box(timestamp.clone());
    }
    let elapsed = start.elapsed().as_nanos() as u64;
    let per_op = elapsed / 1000;

    assert!(
        per_op < TIME_ACCESS_THRESHOLD_NS,
        "Time access too slow: {}ns > {}ns threshold",
        per_op,
        TIME_ACCESS_THRESHOLD_NS
    );
}

/// Quick performance test for timestamp comparison
#[test]
fn test_timestamp_comparison_performance() {
    let ts1 = TimeStamp::PhysicalClock(PhysicalTime {
        ts_ms: 1000000,
        uncertainty: None,
    });
    let ts2 = TimeStamp::PhysicalClock(PhysicalTime {
        ts_ms: 1000001,
        uncertainty: None,
    });

    let start = Instant::now();
    for _ in 0..1000 {
        let _ = std::hint::black_box(ts1.compare(&ts2, OrderingPolicy::Native));
    }
    let elapsed = start.elapsed().as_nanos() as u64;
    let per_op = elapsed / 1000;

    assert!(
        per_op < TIMESTAMP_COMPARE_THRESHOLD_NS,
        "Timestamp comparison too slow: {}ns > {}ns threshold",
        per_op,
        TIMESTAMP_COMPARE_THRESHOLD_NS
    );
}

/// Performance test for optimized VectorClock single device case
#[test]
fn test_vectorclock_single_device_performance() {
    let device = DeviceId::from_bytes([1u8; 32]);

    // Test single device creation performance
    let start = Instant::now();
    for i in 0..1000 {
        let mut clock = VectorClock::new();
        clock.insert(device, 1000 + i);
        std::hint::black_box(clock);
    }
    let elapsed = start.elapsed().as_nanos() as u64;
    let per_op = elapsed / 1000;

    assert!(
        per_op < VECTORCLOCK_SINGLE_THRESHOLD_NS,
        "VectorClock single device too slow: {}ns > {}ns threshold",
        per_op,
        VECTORCLOCK_SINGLE_THRESHOLD_NS
    );
}

/// Performance test for VectorClock multiple device case  
#[test]
fn test_vectorclock_multiple_device_performance() {
    let devices: Vec<_> = (0..5)
        .map(|i| {
            let mut bytes = [0u8; 32];
            bytes[0] = i as u8;
            DeviceId::from_bytes(bytes)
        })
        .collect();

    // Test multiple device creation performance
    let start = Instant::now();
    for _ in 0..100 {
        let mut clock = VectorClock::new();
        for (i, device) in devices.iter().enumerate() {
            clock.insert(*device, 1000 + i as u64);
        }
        std::hint::black_box(clock);
    }
    let elapsed = start.elapsed().as_nanos() as u64;
    let per_op = elapsed / 100;

    assert!(
        per_op < VECTORCLOCK_MULTI_THRESHOLD_NS,
        "VectorClock multiple device too slow: {}ns > {}ns threshold",
        per_op,
        VECTORCLOCK_MULTI_THRESHOLD_NS
    );
}

/// Test that VectorClock optimization provides expected speed-up
#[test]
fn test_vectorclock_optimization_speedup() {
    let single_device = DeviceId::from_bytes([1u8; 32]);
    let multiple_devices: Vec<_> = (0..5)
        .map(|i| {
            let mut bytes = [0u8; 32];
            bytes[0] = i as u8;
            DeviceId::from_bytes(bytes)
        })
        .collect();

    // Measure single device performance
    let start = Instant::now();
    for _ in 0..1000 {
        let mut clock = VectorClock::new();
        clock.insert(single_device, 1000);
        std::hint::black_box(clock);
    }
    let single_elapsed = start.elapsed().as_nanos() as u64;

    // Measure multiple device performance
    let start = Instant::now();
    for _ in 0..1000 {
        let mut clock = VectorClock::new();
        for (i, device) in multiple_devices.iter().enumerate() {
            clock.insert(*device, 1000 + i as u64);
        }
        std::hint::black_box(clock);
    }
    let multi_elapsed = start.elapsed().as_nanos() as u64;

    // Single device should be at least 5x faster than multiple device
    let speedup = multi_elapsed / single_elapsed;
    assert!(
        speedup >= 5,
        "VectorClock optimization not providing expected speedup: {}x < 5x",
        speedup
    );
}

/// Memory usage regression test
#[test]
fn test_memory_usage_bounds() {
    // This is a basic sanity check - actual memory profiling would need external tools

    let device = DeviceId::from_bytes([1u8; 32]);

    // Single device VectorClock should be smaller than multiple device
    let mut single_clock = VectorClock::new();
    single_clock.insert(device, 1000);

    let mut multi_clock = VectorClock::new();
    for i in 0..10 {
        let mut bytes = [0u8; 32];
        bytes[0] = i as u8;
        let device_i = DeviceId::from_bytes(bytes);
        multi_clock.insert(device_i, 1000 + i as u64);
    }

    // At minimum, single device should have fewer entries
    assert!(single_clock.len() < multi_clock.len());

    // Single device should optimize to single variant
    match single_clock {
        VectorClock::Single { .. } => {
            // Expected optimized case
        }
        VectorClock::Multiple(_) => {
            panic!("Single device VectorClock should use optimized Single variant");
        }
    }
}

/// Test sorting performance doesn't regress
#[test]
fn test_sorting_performance() {
    let timestamps: Vec<TimeStamp> = (0..1000)
        .map(|i| {
            TimeStamp::PhysicalClock(PhysicalTime {
                ts_ms: 1000000 + (1000 - i) as u64, // Reverse order for worst case
                uncertainty: None,
            })
        })
        .collect();

    let start = Instant::now();
    let mut to_sort = timestamps.clone();
    to_sort.sort_by(|a, b| a.sort_compare(b, OrderingPolicy::Native));
    let elapsed = start.elapsed();

    // Should sort 1000 timestamps in under 200 microseconds (debug mode)
    assert!(
        elapsed.as_micros() < 200,
        "Sorting 1000 timestamps too slow: {}μs > 200μs threshold",
        elapsed.as_micros()
    );

    // Verify correctness
    for i in 1..to_sort.len() {
        let cmp = to_sort[i - 1].sort_compare(&to_sort[i], OrderingPolicy::Native);
        assert!(
            cmp != std::cmp::Ordering::Greater,
            "Sort order incorrect at position {}",
            i
        );
    }
}
