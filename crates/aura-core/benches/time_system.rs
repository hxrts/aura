#![allow(missing_docs)]
//! Performance benchmarks for the unified time system
//!
//! These benchmarks measure:
//! - Time access latency across different domains (physical, logical, order)
//! - TimeStamp comparison and sorting performance
//! - Cross-domain time conversion overhead
//! - Memory usage of TimeStamp types
//!
//! ## Performance Targets
//! - Time access latency: within 10% of legacy system
//! - Fact ordering performance: within 5% of UUID-based system
//! - Memory overhead: less than 15% increase per fact

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use std::time::Duration;
use tokio::runtime::Runtime;

use aura_core::effects::time::{LogicalClockEffects, OrderClockEffects, PhysicalTimeEffects};
use aura_core::time::{OrderingPolicy, TimeStamp};

// Mock time implementations for consistent benchmarking
struct BenchmarkTimeSource {
    current_time: u64,
}

impl BenchmarkTimeSource {
    fn new(start_time: u64) -> Self {
        Self {
            current_time: start_time,
        }
    }
}

#[async_trait::async_trait]
impl PhysicalTimeEffects for BenchmarkTimeSource {
    async fn physical_time(
        &self,
    ) -> Result<aura_core::time::PhysicalTime, aura_core::effects::time::TimeError> {
        Ok(aura_core::time::PhysicalTime {
            ts_ms: self.current_time,
            uncertainty: None,
        })
    }

    async fn sleep_ms(&self, _ms: u64) -> Result<(), aura_core::effects::time::TimeError> {
        Ok(())
    }
}

#[async_trait::async_trait]
impl LogicalClockEffects for BenchmarkTimeSource {
    async fn logical_now(
        &self,
    ) -> Result<aura_core::time::LogicalTime, aura_core::effects::time::TimeError> {
        let mut clock = aura_core::time::VectorClock::new();
        let device_id = aura_core::identifiers::DeviceId::from_bytes([1u8; 32]);
        clock.insert(device_id, self.current_time);

        Ok(aura_core::time::LogicalTime {
            vector: clock,
            lamport: self.current_time,
        })
    }

    async fn logical_advance(
        &self,
        _observed: Option<&aura_core::time::VectorClock>,
    ) -> Result<aura_core::time::LogicalTime, aura_core::effects::time::TimeError> {
        self.logical_now().await
    }
}

#[async_trait::async_trait]
impl OrderClockEffects for BenchmarkTimeSource {
    async fn order_time(
        &self,
    ) -> Result<aura_core::time::OrderTime, aura_core::effects::time::TimeError> {
        let mut bytes = [0u8; 32];
        bytes[0..8].copy_from_slice(&self.current_time.to_le_bytes());
        Ok(aura_core::time::OrderTime(bytes))
    }
}

/// Benchmark time access latency for different domains
fn bench_time_access_latency(c: &mut Criterion) {
    #[allow(clippy::unwrap_used)]
    let rt = Runtime::new().unwrap();
    let time_source = BenchmarkTimeSource::new(1000000);

    let mut group = c.benchmark_group("time_access_latency");

    group.bench_function("physical_time", |b| {
        b.to_async(&rt).iter(|| async {
            #[allow(clippy::unwrap_used)]
            let time = time_source.physical_time().await.unwrap();
            black_box(time);
        });
    });

    group.bench_function("logical_time", |b| {
        b.to_async(&rt).iter(|| async {
            #[allow(clippy::unwrap_used)]
            let time = time_source.logical_now().await.unwrap();
            black_box(time);
        });
    });

    group.bench_function("order_time", |b| {
        b.to_async(&rt).iter(|| async {
            #[allow(clippy::unwrap_used)]
            let time = time_source.order_time().await.unwrap();
            black_box(time);
        });
    });

    group.finish();
}

/// Benchmark TimeStamp creation from different domains
fn bench_timestamp_creation(c: &mut Criterion) {
    #[allow(clippy::unwrap_used)]
    let rt = Runtime::new().unwrap();
    let time_source = BenchmarkTimeSource::new(1000000);

    let mut group = c.benchmark_group("timestamp_creation");

    group.bench_function("physical_timestamp", |b| {
        b.to_async(&rt).iter(|| async {
            #[allow(clippy::unwrap_used)]
            let physical_time = time_source.physical_time().await.unwrap();
            let timestamp = TimeStamp::PhysicalClock(physical_time);
            black_box(timestamp);
        });
    });

    group.bench_function("logical_timestamp", |b| {
        b.to_async(&rt).iter(|| async {
            #[allow(clippy::unwrap_used)]
            let logical_time = time_source.logical_now().await.unwrap();
            let timestamp = TimeStamp::LogicalClock(logical_time);
            black_box(timestamp);
        });
    });

    group.bench_function("order_timestamp", |b| {
        b.to_async(&rt).iter(|| async {
            #[allow(clippy::unwrap_used)]
            let order_time = time_source.order_time().await.unwrap();
            let timestamp = TimeStamp::OrderClock(order_time);
            black_box(timestamp);
        });
    });

    group.finish();
}

/// Benchmark TimeStamp comparison operations
/// Benchmark TimeStamp comparison operations
fn bench_timestamp_comparison(c: &mut Criterion) {
    #[allow(clippy::unwrap_used)]
    let rt = Runtime::new().unwrap();

    // Pre-generate timestamps for comparison
    let physical_timestamps: Vec<TimeStamp> = rt.block_on(async {
        let mut timestamps = Vec::new();
        for i in 0..1000 {
            let physical_time = aura_core::time::PhysicalTime {
                ts_ms: 1000000 + i,
                uncertainty: None,
            };
            timestamps.push(TimeStamp::PhysicalClock(physical_time));
        }
        timestamps
    });

    let logical_timestamps: Vec<TimeStamp> = rt.block_on(async {
        let mut timestamps = Vec::new();
        for i in 0..1000 {
            let mut clock = aura_core::time::VectorClock::new();
            let device_id = aura_core::identifiers::DeviceId::from_bytes([1u8; 32]);
            clock.insert(device_id, 1000000 + i);

            let logical_time = aura_core::time::LogicalTime {
                vector: clock,
                lamport: 1000000 + i,
            };
            timestamps.push(TimeStamp::LogicalClock(logical_time));
        }
        timestamps
    });

    let mut group = c.benchmark_group("timestamp_comparison");

    // Single comparison benchmarks
    group.bench_function("physical_compare", |b| {
        b.iter(|| {
            let result =
                physical_timestamps[0].compare(&physical_timestamps[1], OrderingPolicy::Native);
            black_box(result);
        });
    });

    group.bench_function("logical_compare", |b| {
        b.iter(|| {
            let result =
                logical_timestamps[0].compare(&logical_timestamps[1], OrderingPolicy::Native);
            black_box(result);
        });
    });

    // Cross-domain comparison (should return Incomparable)
    group.bench_function("cross_domain_compare", |b| {
        b.iter(|| {
            let result =
                physical_timestamps[0].compare(&logical_timestamps[0], OrderingPolicy::Native);
            black_box(result);
        });
    });

    group.finish();
}

/// Benchmark TimeStamp sorting operations (critical for fact ordering)
fn bench_timestamp_sorting(c: &mut Criterion) {
    #[allow(clippy::unwrap_used)]
    let _rt = Runtime::new().unwrap();

    // Pre-generate different sized collections of timestamps
    let sizes = [10, 100, 1000];

    for size in sizes {
        let physical_timestamps: Vec<TimeStamp> = (0..size)
            .map(|i| {
                let physical_time = aura_core::time::PhysicalTime {
                    ts_ms: 1000000 + (size - i) as u64, // Reverse order for worst-case sorting
                    uncertainty: None,
                };
                TimeStamp::PhysicalClock(physical_time)
            })
            .collect();

        let mixed_timestamps: Vec<TimeStamp> = (0..size)
            .map(|i| {
                if i % 2 == 0 {
                    let physical_time = aura_core::time::PhysicalTime {
                        ts_ms: 1000000 + i as u64,
                        uncertainty: None,
                    };
                    TimeStamp::PhysicalClock(physical_time)
                } else {
                    let mut clock = aura_core::time::VectorClock::new();
                    let device_id = aura_core::identifiers::DeviceId::from_bytes([1u8; 32]);
                    clock.insert(device_id, 1000000 + i as u64);

                    let logical_time = aura_core::time::LogicalTime {
                        vector: clock,
                        lamport: 1000000 + i as u64,
                    };
                    TimeStamp::LogicalClock(logical_time)
                }
            })
            .collect();

        c.bench_with_input(
            BenchmarkId::new("sort_physical_timestamps", size),
            &physical_timestamps,
            |b, timestamps| {
                b.iter(|| {
                    let mut to_sort = timestamps.clone();
                    to_sort.sort_by(|a, b| a.sort_compare(b, OrderingPolicy::Native));
                    black_box(to_sort);
                });
            },
        );

        c.bench_with_input(
            BenchmarkId::new("sort_mixed_timestamps", size),
            &mixed_timestamps,
            |b, timestamps| {
                b.iter(|| {
                    let mut to_sort = timestamps.clone();
                    to_sort.sort_by(|a, b| a.sort_compare(b, OrderingPolicy::Native));
                    black_box(to_sort);
                });
            },
        );

        // Benchmark optimized collection sorting
        c.bench_with_input(
            BenchmarkId::new("optimized_sort_physical_timestamps", size),
            &physical_timestamps,
            |b, timestamps| {
                b.iter(|| {
                    let mut to_sort = timestamps.clone();
                    TimeStamp::sort_collection_optimized(
                        &mut to_sort,
                        OrderingPolicy::Native,
                        false,
                    );
                    black_box(to_sort);
                });
            },
        );

        c.bench_with_input(
            BenchmarkId::new("optimized_sort_mixed_timestamps", size),
            &mixed_timestamps,
            |b, timestamps| {
                b.iter(|| {
                    let mut to_sort = timestamps.clone();
                    TimeStamp::sort_collection_optimized(
                        &mut to_sort,
                        OrderingPolicy::Native,
                        false,
                    );
                    black_box(to_sort);
                });
            },
        );
    }
}

/// Benchmark unified time utility methods
fn bench_time_utilities(c: &mut Criterion) {
    let physical_timestamp = TimeStamp::PhysicalClock(aura_core::time::PhysicalTime {
        ts_ms: 1000000,
        uncertainty: None,
    });

    let logical_timestamp = {
        let mut clock = aura_core::time::VectorClock::new();
        let device_id = aura_core::identifiers::DeviceId::from_bytes([1u8; 32]);
        clock.insert(device_id, 1000000);

        TimeStamp::LogicalClock(aura_core::time::LogicalTime {
            vector: clock,
            lamport: 1000000,
        })
    };

    let mut group = c.benchmark_group("time_utilities");

    group.bench_function("to_index_ms_physical", |b| {
        b.iter(|| {
            let index = physical_timestamp.to_index_ms();
            black_box(index);
        });
    });

    group.bench_function("to_index_ms_logical", |b| {
        b.iter(|| {
            let index = logical_timestamp.to_index_ms();
            black_box(index);
        });
    });

    group.finish();
}

/// Benchmark memory usage patterns (measures allocation overhead)
fn bench_memory_usage(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_usage");
    group.measurement_time(Duration::from_secs(10));

    // Benchmark creating many timestamps to measure memory overhead
    group.bench_function("create_1000_physical_timestamps", |b| {
        b.iter(|| {
            let timestamps: Vec<TimeStamp> = (0..1000)
                .map(|i| {
                    let physical_time = aura_core::time::PhysicalTime {
                        ts_ms: 1000000 + i,
                        uncertainty: None,
                    };
                    TimeStamp::PhysicalClock(physical_time)
                })
                .collect();
            black_box(timestamps);
        });
    });

    group.bench_function("create_1000_logical_timestamps", |b| {
        b.iter(|| {
            let timestamps: Vec<TimeStamp> = (0..1000)
                .map(|i| {
                    let mut clock = aura_core::time::VectorClock::new();
                    let device_id = aura_core::identifiers::DeviceId::from_bytes([1u8; 32]);
                    clock.insert(device_id, 1000000 + i);

                    let logical_time = aura_core::time::LogicalTime {
                        vector: clock,
                        lamport: 1000000 + i,
                    };
                    TimeStamp::LogicalClock(logical_time)
                })
                .collect();
            black_box(timestamps);
        });
    });

    group.finish();
}

/// Benchmark VectorClock optimization performance
fn bench_vectorclock_optimization(c: &mut Criterion) {
    let mut group = c.benchmark_group("vectorclock_optimization");

    // Single device case (optimized path)
    let single_device = aura_core::identifiers::DeviceId::from_bytes([1u8; 32]);

    // Multiple devices case (fallback path)
    let devices: Vec<_> = (0..10)
        .map(|i| {
            let mut bytes = [0u8; 32];
            bytes[0] = i as u8;
            aura_core::identifiers::DeviceId::from_bytes(bytes)
        })
        .collect();

    // Benchmark single device creation
    group.bench_function("single_device_creation", |b| {
        b.iter(|| {
            let mut clock = aura_core::time::VectorClock::new();
            clock.insert(single_device, 1000);
            black_box(clock);
        });
    });

    // Benchmark multiple device creation
    group.bench_function("multiple_device_creation", |b| {
        b.iter(|| {
            let mut clock = aura_core::time::VectorClock::new();
            for (i, device) in devices.iter().enumerate() {
                clock.insert(*device, 1000 + i as u64);
            }
            black_box(clock);
        });
    });

    // Pre-create clocks for comparison benchmarks
    let mut single_clock1 = aura_core::time::VectorClock::new();
    single_clock1.insert(single_device, 1000);

    let mut single_clock2 = aura_core::time::VectorClock::new();
    single_clock2.insert(single_device, 1001);

    let mut multi_clock1 = aura_core::time::VectorClock::new();
    for (i, device) in devices.iter().enumerate() {
        multi_clock1.insert(*device, 1000 + i as u64);
    }

    let mut multi_clock2 = aura_core::time::VectorClock::new();
    for (i, device) in devices.iter().enumerate() {
        multi_clock2.insert(*device, 1001 + i as u64);
    }

    // Benchmark single device comparison (fast path)
    group.bench_function("single_device_comparison", |b| {
        b.iter(|| {
            let result = single_clock1.partial_cmp(&single_clock2);
            black_box(result);
        });
    });

    // Benchmark multiple device comparison
    group.bench_function("multiple_device_comparison", |b| {
        b.iter(|| {
            let result = multi_clock1.partial_cmp(&multi_clock2);
            black_box(result);
        });
    });

    // Memory usage benchmarks
    group.bench_function("single_device_memory_1000", |b| {
        b.iter(|| {
            let clocks: Vec<_> = (0..1000)
                .map(|i| {
                    let mut clock = aura_core::time::VectorClock::new();
                    clock.insert(single_device, 1000 + i);
                    clock
                })
                .collect();
            black_box(clocks);
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_time_access_latency,
    bench_timestamp_creation,
    bench_timestamp_comparison,
    bench_timestamp_sorting,
    bench_time_utilities,
    bench_memory_usage,
    bench_vectorclock_optimization
);

criterion_main!(benches);
