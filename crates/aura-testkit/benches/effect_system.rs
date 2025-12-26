//! Performance benchmarks for effect system initialization and basic operations
//!
//! These benchmarks measure:
//! - Effect system initialization time
//! - Handler creation and composition
//! - Basic effect execution overhead

#![allow(clippy::expect_used, missing_docs, unused_attributes)]

use criterion::{black_box, criterion_group, criterion_main, Criterion};

use aura_core::DeviceId;
use aura_testkit::infrastructure::effects::TestEffectsBuilder;

/// Benchmark effect system initialization for testing mode
fn bench_initialization(c: &mut Criterion) {
    c.bench_function("effect_system_testing", |b| {
        b.iter(|| {
            let device_id = DeviceId::from_bytes([1u8; 32]);
            let system = TestEffectsBuilder::for_unit_tests(device_id)
                .build()
                .expect("Failed to build test effect system");
            black_box(system);
        });
    });

    c.bench_function("effect_system_simulation", |b| {
        b.iter(|| {
            let device_id = DeviceId::from_bytes([3u8; 32]);
            let system = TestEffectsBuilder::for_simulation(device_id)
                .with_seed(42)
                .build()
                .expect("Failed to build simulation effect system");
            black_box(system);
        });
    });
}

/// Benchmark registry configuration with optional features
fn bench_registry_configuration(c: &mut Criterion) {
    c.bench_function("registry_basic_config", |b| {
        b.iter(|| {
            let device_id = DeviceId::from_bytes([4u8; 32]);
            let system = TestEffectsBuilder::for_unit_tests(device_id)
                .build()
                .expect("Failed to build test effect system");
            black_box(system);
        });
    });

    c.bench_function("registry_with_features", |b| {
        b.iter(|| {
            let device_id = DeviceId::from_bytes([5u8; 32]);
            let system = TestEffectsBuilder::for_unit_tests(device_id)
                .build()
                .expect("Failed to build test effect system");
            black_box(system);
        });
    });
}

/// Benchmark effect system creation with different configurations
fn bench_effect_execution(c: &mut Criterion) {
    c.bench_function("testing_config_full_build", |b| {
        b.iter(|| {
            let device_id = DeviceId::from_bytes([7u8; 32]);
            let effect_system = TestEffectsBuilder::for_unit_tests(device_id)
                .build()
                .expect("Failed to build test effect system");
            black_box(effect_system);
        });
    });

    c.bench_function("simulation_config_full_build", |b| {
        b.iter(|| {
            let device_id = DeviceId::from_bytes([8u8; 32]);
            let effect_system = TestEffectsBuilder::for_simulation(device_id)
                .build()
                .expect("Failed to build simulation effect system");
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
