//! Performance benchmarks for effect system initialization and basic operations
//!
//! These benchmarks measure:
//! - Effect system initialization time
//! - Handler creation and composition
//! - Basic effect execution overhead

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use tokio::runtime::Runtime;

use aura_core::identifiers::DeviceId;
use aura_agent::runtime::EffectRegistry;

/// Benchmark effect system initialization for different modes
fn bench_initialization(c: &mut Criterion) {
    c.bench_function("effect_system_testing", |b| {
        b.iter(|| {
            let device_id = DeviceId::from_bytes([1u8; 32]);
            let system = EffectRegistry::testing()
                .with_device_id(device_id)
                .build()
                .unwrap();
            black_box(system);
        });
    });

    c.bench_function("effect_system_production", |b| {
        b.iter(|| {
            let device_id = DeviceId::from_bytes([2u8; 32]);
            let system = EffectRegistry::production()
                .with_device_id(device_id)
                .build()
                .unwrap();
            black_box(system);
        });
    });

    c.bench_function("effect_system_simulation", |b| {
        b.iter(|| {
            let device_id = DeviceId::from_bytes([3u8; 32]);
            let system = EffectRegistry::simulation(42)
                .with_device_id(device_id)
                .build()
                .unwrap();
            black_box(system);
        });
    });
}

/// Benchmark registry configuration with optional features
fn bench_registry_configuration(c: &mut Criterion) {
    c.bench_function("registry_basic_config", |b| {
        b.iter(|| {
            let device_id = DeviceId::from_bytes([4u8; 32]);
            let system = EffectRegistry::testing()
                .with_device_id(device_id)
                .build()
                .unwrap();
            black_box(system);
        });
    });

    c.bench_function("registry_with_logging", |b| {
        b.iter(|| {
            let device_id = DeviceId::from_bytes([5u8; 32]);
            let system = EffectRegistry::testing()
                .with_device_id(device_id)
                .with_logging()
                .build()
                .unwrap();
            black_box(system);
        });
    });

    c.bench_function("registry_with_all_features", |b| {
        b.iter(|| {
            let device_id = DeviceId::from_bytes([6u8; 32]);
            let system = EffectRegistry::testing()
                .with_device_id(device_id)
                .with_logging()
                .with_metrics()
                .with_tracing()
                .build()
                .unwrap();
            black_box(system);
        });
    });
}

/// Benchmark effect system creation with different configurations
fn bench_effect_execution(c: &mut Criterion) {
    c.bench_function("testing_config_full_build", |b| {
        b.iter(|| {
            let device_id = DeviceId::from_bytes([7u8; 32]);
            let effect_system = EffectRegistry::testing()
                .with_device_id(device_id)
                .build()
                .unwrap();
            black_box(effect_system);
        });
    });

    c.bench_function("production_config_full_build", |b| {
        b.iter(|| {
            let device_id = DeviceId::from_bytes([8u8; 32]);
            let effect_system = EffectRegistry::production()
                .with_device_id(device_id)
                .build()
                .unwrap();
            black_box(effect_system);
        });
    });
}

criterion_group!(
    benches,
    bench_initialization,
    bench_registry_configuration,
    bench_effect_execution
);

criterion_main!(benches);
