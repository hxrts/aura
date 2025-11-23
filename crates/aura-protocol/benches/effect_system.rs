//! Performance benchmarks for effect system initialization and basic operations
//!
//! These benchmarks measure:
//! - Effect system initialization time
//! - Handler creation and composition
//! - Basic effect execution overhead

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use tokio::runtime::Runtime;

use aura_core::identifiers::DeviceId;
use aura_testkit::effect_system::TestEffectRegistry;

/// Benchmark effect system initialization for testing mode
fn bench_initialization(c: &mut Criterion) {
    c.bench_function("effect_system_testing", |b| {
        b.iter(|| {
            let device_id = DeviceId::from_bytes([1u8; 32]);
            let system = TestEffectRegistry::new()
                .with_device_id(device_id)
                .build()
                .unwrap();
            black_box(system);
        });
    });

    c.bench_function("effect_system_simulation", |b| {
        b.iter(|| {
            let device_id = DeviceId::from_bytes([3u8; 32]);
            let system = TestEffectRegistry::new_simulation(42)
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
            let system = TestEffectRegistry::new()
                .with_device_id(device_id)
                .build()
                .unwrap();
            black_box(system);
        });
    });

    c.bench_function("registry_with_features", |b| {
        b.iter(|| {
            let device_id = DeviceId::from_bytes([5u8; 32]);
            let system = TestEffectRegistry::new()
                .with_device_id(device_id)
                .with_deterministic_time()
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
            let effect_system = TestEffectRegistry::new()
                .with_device_id(device_id)
                .build()
                .unwrap();
            black_box(effect_system);
        });
    });

    c.bench_function("mock_config_full_build", |b| {
        b.iter(|| {
            let device_id = DeviceId::from_bytes([8u8; 32]);
            let effect_system = TestEffectRegistry::new_mock()
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
