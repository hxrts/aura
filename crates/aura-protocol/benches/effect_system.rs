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
    AuraEffectSystem, AuraEffectSystemBuilder, CryptoEffects, EffectSystemConfig, NetworkEffects,
    StorageEffects, TimeEffects,
};
use std::sync::Arc;
use std::time::Duration;

/// Benchmark effect system initialization
fn bench_initialization(c: &mut Criterion) {
    let runtime = Runtime::new().unwrap();

    c.bench_function("effect_system_new_sync", |b| {
        b.iter(|| {
            let system = AuraEffectSystem::for_testing_sync();
            black_box(system);
        });
    });

    c.bench_function("effect_system_new_async", |b| {
        b.to_async(&runtime).iter(|| async {
            let system = AuraEffectSystem::new().await;
            black_box(system);
        });
    });

    c.bench_function("effect_system_builder_minimal", |b| {
        b.to_async(&runtime).iter(|| async {
            let builder = AuraEffectSystemBuilder::new()
                .with_network_handler(Arc::new(MockNetworkHandler::new()))
                .build()
                .await;
            black_box(builder);
        });
    });

    c.bench_function("effect_system_builder_full", |b| {
        b.to_async(&runtime).iter(|| async {
            let builder = AuraEffectSystemBuilder::new()
                .with_network_handler(Arc::new(MockNetworkHandler::new()))
                .with_storage_handler(Arc::new(InMemoryStorageHandler::new()))
                .with_crypto_handler(Arc::new(MockCryptoHandler::new()))
                .with_time_handler(Arc::new(MockTimeHandler::new()))
                .with_config(EffectSystemConfig::default())
                .build()
                .await;
            black_box(builder);
        });
    });
}

/// Benchmark handler registration performance
fn bench_handler_registration(c: &mut Criterion) {
    let runtime = Runtime::new().unwrap();

    c.bench_function("register_single_handler", |b| {
        b.to_async(&runtime).iter_batched(
            || AuraEffectSystemBuilder::new(),
            |builder| async move {
                builder
                    .with_network_handler(Arc::new(MockNetworkHandler::new()))
                    .build()
                    .await
            },
            BatchSize::SmallInput,
        );
    });

    c.bench_function("register_multiple_handlers", |b| {
        b.to_async(&runtime).iter_batched(
            || AuraEffectSystemBuilder::new(),
            |builder| async move {
                builder
                    .with_network_handler(Arc::new(MockNetworkHandler::new()))
                    .with_storage_handler(Arc::new(InMemoryStorageHandler::new()))
                    .with_crypto_handler(Arc::new(MockCryptoHandler::new()))
                    .with_time_handler(Arc::new(MockTimeHandler::new()))
                    .build()
                    .await
            },
            BatchSize::SmallInput,
        );
    });
}

/// Benchmark effect execution overhead
fn bench_effect_execution(c: &mut Criterion) {
    let runtime = Runtime::new().unwrap();

    // Setup effect system once for execution benchmarks
    let effect_system = runtime.block_on(async { Arc::new(AuraEffectSystem::new().await) });

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
    let effect_system = runtime.block_on(async { Arc::new(AuraEffectSystem::new().await) });

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
                let system = AuraEffectSystem::new().await;
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

/// Benchmark configuration variations
fn bench_configurations(c: &mut Criterion) {
    let runtime = Runtime::new().unwrap();

    c.bench_function("default_config", |b| {
        b.to_async(&runtime).iter(|| async {
            let config = EffectSystemConfig::default();
            let system = AuraEffectSystemBuilder::new()
                .with_config(config)
                .build()
                .await;
            black_box(system);
        });
    });

    c.bench_function("minimal_config", |b| {
        b.to_async(&runtime).iter(|| async {
            let config = EffectSystemConfig {
                enable_tracing: false,
                enable_metrics: false,
                max_concurrent_effects: 10,
                effect_timeout: Duration::from_secs(30),
                ..Default::default()
            };
            let system = AuraEffectSystemBuilder::new()
                .with_config(config)
                .build()
                .await;
            black_box(system);
        });
    });

    c.bench_function("maximal_config", |b| {
        b.to_async(&runtime).iter(|| async {
            let config = EffectSystemConfig {
                enable_tracing: true,
                enable_metrics: true,
                max_concurrent_effects: 1000,
                effect_timeout: Duration::from_secs(300),
                ..Default::default()
            };
            let system = AuraEffectSystemBuilder::new()
                .with_config(config)
                .build()
                .await;
            black_box(system);
        });
    });
}

criterion_group!(
    benches,
    bench_initialization,
    bench_handler_registration,
    bench_effect_execution,
    bench_effect_batching,
    bench_memory_patterns,
    bench_configurations
);

criterion_main!(benches);
