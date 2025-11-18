//! Performance benchmarks for AuraEffectSystem initialization and operations
//!
//! These benchmarks measure:
//! - Effect system initialization time
//! - Handler registration performance
//! - Effect execution overhead
//! - Memory usage patterns

use criterion::{black_box, criterion_group, criterion_main, BatchSize, Criterion};
use tokio::runtime::Runtime;

use aura_core::{DeviceId, FlowBudget};
use aura_effects::handlers::{
    InMemoryStorageHandler, MockCryptoHandler, MockNetworkHandler, MockTimeHandler,
};
use aura_protocol::effects::{
    AuraEffectSystem, CryptoEffects, EffectRegistry, NetworkEffects, StorageEffects, TimeEffects,
};
use std::sync::Arc;
use std::time::Duration;

/// Benchmark effect system initialization
fn bench_initialization(c: &mut Criterion) {
    let runtime = Runtime::new().unwrap();

    c.bench_function("effect_system_testing_registry", |b| {
        b.iter(|| {
            let device_id = DeviceId::new();
            let system = EffectRegistry::testing()
                .with_device_id(device_id)
                .build()
                .unwrap();
            black_box(system);
        });
    });

    c.bench_function("effect_system_production_registry", |b| {
        b.iter(|| {
            let device_id = DeviceId::new();
            let system = EffectRegistry::production()
                .with_device_id(device_id)
                .build()
                .unwrap();
            black_box(system);
        });
    });

    c.bench_function("effect_system_simulation_registry", |b| {
        b.iter(|| {
            let device_id = DeviceId::new();
            let system = EffectRegistry::simulation(42)
                .with_device_id(device_id)
                .with_logging()
                .build()
                .unwrap();
            black_box(system);
        });
    });
}

/// Benchmark registry configuration performance
fn bench_registry_configuration(c: &mut Criterion) {
    c.bench_function("registry_testing_config", |b| {
        b.iter(|| {
            let device_id = DeviceId::new();
            let system = EffectRegistry::testing()
                .with_device_id(device_id)
                .build()
                .unwrap();
            black_box(system);
        });
    });

    c.bench_function("registry_production_config", |b| {
        b.iter(|| {
            let device_id = DeviceId::new();
            let system = EffectRegistry::production()
                .with_device_id(device_id)
                .with_logging()
                .with_metrics()
                .build()
                .unwrap();
            black_box(system);
        });
    });
}

/// Benchmark effect execution overhead
fn bench_effect_execution(c: &mut Criterion) {
    let runtime = Runtime::new().unwrap();

    // Setup effect system once for execution benchmarks
    let device_id = DeviceId::new();
    let effect_system = Arc::new(
        EffectRegistry::testing()
            .with_device_id(device_id)
            .build()
            .unwrap(),
    );

    c.bench_function("network_send_overhead", |b| {
        let system = effect_system.clone();
        b.to_async(&runtime).iter(|| async {
            let peer_id = DeviceId::new();
            let message = vec![0u8; 1024]; // 1KB message
            let _ = system.send_to_peer(peer_id, message).await;
        });
    });

    c.bench_function("storage_store_overhead", |b| {
        let system = effect_system.clone();
        b.to_async(&runtime).iter(|| async {
            let key = "bench_key";
            let value = vec![0u8; 4096]; // 4KB value
            let _ = system.store(key, value, false).await;
        });
    });

    c.bench_function("crypto_sign_overhead", |b| {
        let system = effect_system.clone();
        b.to_async(&runtime).iter(|| async {
            let message = b"benchmark message";
            let private_key = vec![0u8; 32];
            let _ = system.ed25519_sign(message, &private_key).await;
        });
    });

    c.bench_function("time_current_epoch", |b| {
        let system = effect_system.clone();
        b.to_async(&runtime).iter(|| async {
            let _ = system.current_epoch().await;
        });
    });
}

/// Benchmark effect batching and parallelization
fn bench_effect_batching(c: &mut Criterion) {
    let runtime = Runtime::new().unwrap();
    let device_id = DeviceId::new();
    let effect_system = Arc::new(
        EffectRegistry::testing()
            .with_device_id(device_id)
            .build()
            .unwrap(),
    );

    c.bench_function("sequential_network_calls", |b| {
        let system = effect_system.clone();
        b.to_async(&runtime).iter(|| async {
            let peer_id = DeviceId::new();
            for _ in 0..10 {
                let _ = system.send_to_peer(peer_id, vec![0u8; 256]).await;
            }
        });
    });

    c.bench_function("parallel_network_calls", |b| {
        let system = effect_system.clone();
        b.to_async(&runtime).iter(|| async {
            let peer_id = DeviceId::new();
            let mut handles = vec![];

            for _ in 0..10 {
                let sys = system.clone();
                let handle = tokio::spawn(async move {
                    let _ = sys.send_to_peer(peer_id, vec![0u8; 256]).await;
                });
                handles.push(handle);
            }

            for handle in handles {
                let _ = handle.await;
            }
        });
    });

    c.bench_function("mixed_effect_calls", |b| {
        let system = effect_system.clone();
        b.to_async(&runtime).iter(|| async {
            let peer_id = DeviceId::new();

            // Mix of different effect calls
            let _ = system.send_to_peer(peer_id, vec![0u8; 256]).await;
            let _ = system.store("key", vec![0u8; 512], false).await;
            let _ = system.current_epoch().await;
            let _ = system.ed25519_sign(b"msg", &vec![0u8; 32]).await;
            let _ = system.recv_from_peer(peer_id).await;
        });
    });
}

/// Benchmark memory allocation patterns
fn bench_memory_patterns(c: &mut Criterion) {
    let runtime = Runtime::new().unwrap();

    c.bench_function("effect_system_memory_footprint", |b| {
        b.to_async(&runtime).iter_batched(
            || {},
            |_| async {
                let device_id = DeviceId::new();
                let system = EffectRegistry::testing()
                    .with_device_id(device_id)
                    .build()
                    .unwrap();
                // Force system to be used to prevent optimization
                let _ = system.current_epoch().await;
                black_box(system);
            },
            BatchSize::SmallInput,
        );
    });

    c.bench_function("handler_allocation_overhead", |b| {
        b.iter(|| {
            let handlers: Vec<Arc<dyn NetworkEffects>> = (0..100)
                .map(|_| Arc::new(MockNetworkHandler::new()) as Arc<dyn NetworkEffects>)
                .collect();
            black_box(handlers);
        });
    });
}

/// Benchmark registry configuration variations
fn bench_registry_variations(c: &mut Criterion) {
    c.bench_function("registry_minimal_config", |b| {
        b.iter(|| {
            let device_id = DeviceId::new();
            let system = EffectRegistry::testing()
                .with_device_id(device_id)
                .build()
                .unwrap();
            black_box(system);
        });
    });

    c.bench_function("registry_with_logging", |b| {
        b.iter(|| {
            let device_id = DeviceId::new();
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
            let device_id = DeviceId::new();
            let system = EffectRegistry::production()
                .with_device_id(device_id)
                .with_logging()
                .with_metrics()
                .build()
                .unwrap();
            black_box(system);
        });
    });
}

criterion_group!(
    benches,
    bench_initialization,
    bench_registry_configuration,
    bench_effect_execution,
    bench_effect_batching,
    bench_memory_patterns,
    bench_registry_variations
);

criterion_main!(benches);
