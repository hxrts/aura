#![allow(missing_docs)]
//! VectorClock performance regression benchmarks
//!
//! These benchmarks detect performance regressions in VectorClock operations.
//! Run with: `cargo bench -p aura-core`
//!
//! Key metrics:
//! - Single device optimization: creation, increment, get
//! - Multiple device: creation, comparison
//! - Sorting performance for timestamps

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use std::collections::BTreeMap;

use aura_core::time::{OrderingPolicy, PhysicalTime, TimeStamp, VectorClock};
use aura_core::types::identifiers::DeviceId;

/// Benchmark VectorClock single device operations (optimized path)
fn bench_vectorclock_single_device(c: &mut Criterion) {
    let device = DeviceId::from_bytes([1u8; 32]);

    let mut group = c.benchmark_group("vectorclock_single_device");

    // Creation benchmark
    group.bench_function("creation", |b| {
        b.iter(|| {
            let clock = VectorClock::single(black_box(device), 1000);
            black_box(clock)
        });
    });

    // Increment benchmark
    group.bench_function("increment", |b| {
        let mut clock = VectorClock::single(device, 0);
        b.iter(|| {
            clock
                .increment(black_box(device))
                .expect("vector clock increment");
        });
        black_box(clock);
    });

    // Get benchmark
    group.bench_function("get", |b| {
        let clock = VectorClock::single(device, 1000);
        b.iter(|| {
            let val = clock.get(&device);
            black_box(val)
        });
    });

    group.finish();
}

/// Benchmark VectorClock multiple device operations
fn bench_vectorclock_multiple_device(c: &mut Criterion) {
    let devices: Vec<_> = (0..5)
        .map(|i| {
            let mut bytes = [0u8; 32];
            bytes[0] = i as u8;
            DeviceId::from_bytes(bytes)
        })
        .collect();

    let mut group = c.benchmark_group("vectorclock_multiple_device");

    // Creation with insertions
    group.bench_function("creation_5_devices", |b| {
        b.iter(|| {
            let mut clock = VectorClock::Multiple(BTreeMap::new());
            for (i, device) in devices.iter().enumerate() {
                clock.insert(*device, 1000 + i as u64);
            }
            black_box(clock)
        });
    });

    // Comparison benchmark
    group.bench_function("comparison", |b| {
        let mut clock1 = VectorClock::Multiple(BTreeMap::new());
        let mut clock2 = VectorClock::Multiple(BTreeMap::new());
        for (i, device) in devices.iter().enumerate() {
            clock1.insert(*device, 1000 + i as u64);
            clock2.insert(*device, 1001 + i as u64);
        }
        b.iter(|| {
            let result = clock1.partial_cmp(&clock2);
            black_box(result)
        });
    });

    group.finish();
}

/// Benchmark timestamp operations
fn bench_timestamp_operations(c: &mut Criterion) {
    let ts1 = TimeStamp::PhysicalClock(PhysicalTime {
        ts_ms: 1000000,
        uncertainty: None,
    });
    let ts2 = TimeStamp::PhysicalClock(PhysicalTime {
        ts_ms: 1000001,
        uncertainty: None,
    });

    let mut group = c.benchmark_group("timestamp_operations");

    // Clone/access benchmark
    group.bench_function("clone", |b| {
        b.iter(|| {
            let cloned = ts1.clone();
            black_box(cloned)
        });
    });

    // Comparison benchmark
    group.bench_function("compare", |b| {
        b.iter(|| {
            let result = ts1.compare(&ts2, OrderingPolicy::Native);
            black_box(result)
        });
    });

    group.finish();
}

/// Benchmark timestamp sorting (critical for fact ordering)
fn bench_timestamp_sorting(c: &mut Criterion) {
    let sizes = [100, 500, 1000];

    for size in sizes {
        let timestamps: Vec<TimeStamp> = (0..size)
            .map(|i| {
                TimeStamp::PhysicalClock(PhysicalTime {
                    ts_ms: 1000000 + (size - i) as u64, // Reverse order for worst case
                    uncertainty: None,
                })
            })
            .collect();

        c.bench_with_input(
            BenchmarkId::new("sort_timestamps", size),
            &timestamps,
            |b, timestamps| {
                b.iter(|| {
                    let mut to_sort = timestamps.clone();
                    to_sort.sort_by(|a, b| a.sort_compare(b, OrderingPolicy::Native));
                    black_box(to_sort)
                });
            },
        );

        c.bench_with_input(
            BenchmarkId::new("sort_timestamps_optimized", size),
            &timestamps,
            |b, timestamps| {
                b.iter(|| {
                    let mut to_sort = timestamps.clone();
                    TimeStamp::sort_collection_optimized(
                        &mut to_sort,
                        OrderingPolicy::Native,
                        false,
                    );
                    black_box(to_sort)
                });
            },
        );
    }
}

/// Benchmark optimization speedup comparison
fn bench_optimization_speedup(c: &mut Criterion) {
    let single_device = DeviceId::from_bytes([1u8; 32]);
    let multiple_devices: Vec<_> = (0..5)
        .map(|i| {
            let mut bytes = [0u8; 32];
            bytes[0] = i as u8;
            DeviceId::from_bytes(bytes)
        })
        .collect();

    let mut group = c.benchmark_group("optimization_comparison");

    // Single device creation (should be faster)
    group.bench_function("single_device_creation", |b| {
        b.iter(|| {
            let clock = VectorClock::single(single_device, 1000);
            black_box(clock)
        });
    });

    // Multiple device creation (baseline)
    group.bench_function("multi_device_creation", |b| {
        b.iter(|| {
            let mut clock = VectorClock::Multiple(BTreeMap::new());
            for (i, device) in multiple_devices.iter().enumerate() {
                clock.insert(*device, 1000 + i as u64);
            }
            black_box(clock)
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_vectorclock_single_device,
    bench_vectorclock_multiple_device,
    bench_timestamp_operations,
    bench_timestamp_sorting,
    bench_optimization_speedup,
);

criterion_main!(benches);
