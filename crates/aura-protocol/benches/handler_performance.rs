//! Performance benchmarks for effect handlers
//!
//! These benchmarks measure:
//! - Handler invocation overhead
//! - Handler composition performance
//! - Middleware stack overhead
//! - Concurrent handler access

use criterion::{black_box, criterion_group, criterion_main, BatchSize, Criterion};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::runtime::Runtime;

use aura_core::{AuraError, DeviceId};
use aura_effects::handlers::{
    InMemoryStorageHandler, MockCryptoHandler, MockNetworkHandler, MockTimeHandler,
};
use aura_protocol::{
    effects::{
        container::EffectContainer,
        executor::{EffectExecutor, EffectExecutorBuilder},
        AuraEffectSystem, CryptoEffects, NetworkEffects, StorageEffects, TimeEffects,
    },
    handlers::{factory::HandlerFactory, registry::HandlerRegistry, CompositeHandler},
};

/// Benchmark raw handler invocation
fn bench_handler_invocation(c: &mut Criterion) {
    let runtime = Runtime::new().unwrap();

    c.bench_function("direct_handler_call", |b| {
        let handler = MockNetworkHandler::new();
        b.to_async(&runtime).iter(|| async {
            let _ = handler.send_to_peer(DeviceId::new(), vec![0u8; 256]).await;
        });
    });

    c.bench_function("trait_object_call", |b| {
        let handler: Arc<dyn NetworkEffects> = Arc::new(MockNetworkHandler::new());
        b.to_async(&runtime).iter(|| async {
            let _ = handler.send_to_peer(DeviceId::new(), vec![0u8; 256]).await;
        });
    });

    c.bench_function("arc_clone_overhead", |b| {
        let handler: Arc<dyn NetworkEffects> = Arc::new(MockNetworkHandler::new());
        b.iter(|| {
            let cloned = handler.clone();
            black_box(cloned);
        });
    });
}

/// Benchmark handler composition
fn bench_handler_composition(c: &mut Criterion) {
    let runtime = Runtime::new().unwrap();

    c.bench_function("composite_handler_single", |b| {
        let composite = CompositeHandler::new().with_network(Arc::new(MockNetworkHandler::new()));

        b.to_async(&runtime).iter(|| async {
            let _ = composite
                .send_to_peer(DeviceId::new(), vec![0u8; 256])
                .await;
        });
    });

    c.bench_function("composite_handler_full", |b| {
        let composite = CompositeHandler::new()
            .with_network(Arc::new(MockNetworkHandler::new()))
            .with_storage(Arc::new(InMemoryStorageHandler::new()))
            .with_crypto(Arc::new(MockCryptoHandler::new()))
            .with_time(Arc::new(MockTimeHandler::new()));

        b.to_async(&runtime).iter(|| async {
            // Mix of operations
            let _ = composite
                .send_to_peer(DeviceId::new(), vec![0u8; 256])
                .await;
            let _ = composite.store("key", vec![0u8; 512], false).await;
            let _ = composite.current_epoch().await;
        });
    });
}

/// Benchmark handler registry operations
fn bench_handler_registry(c: &mut Criterion) {
    c.bench_function("registry_insert", |b| {
        b.iter_batched(
            || HandlerRegistry::new(),
            |mut registry| {
                for i in 0..100 {
                    registry.register_network_handler(
                        format!("handler_{}", i),
                        Arc::new(MockNetworkHandler::new()),
                    );
                }
                black_box(registry);
            },
            BatchSize::SmallInput,
        );
    });

    c.bench_function("registry_lookup", |b| {
        let mut registry = HandlerRegistry::new();
        for i in 0..100 {
            registry.register_network_handler(
                format!("handler_{}", i),
                Arc::new(MockNetworkHandler::new()),
            );
        }

        b.iter(|| {
            let handler = registry.get_network_handler("handler_50");
            black_box(handler);
        });
    });

    c.bench_function("registry_iteration", |b| {
        let mut registry = HandlerRegistry::new();
        for i in 0..100 {
            registry.register_network_handler(
                format!("handler_{}", i),
                Arc::new(MockNetworkHandler::new()),
            );
        }

        b.iter(|| {
            let count = registry.network_handlers().count();
            black_box(count);
        });
    });
}

/// Benchmark middleware stack overhead
fn bench_middleware_stack(c: &mut Criterion) {
    let runtime = Runtime::new().unwrap();

    // Simple counting middleware
    struct CountingMiddleware {
        inner: Arc<dyn NetworkEffects>,
        count: Arc<AtomicU64>,
    }

    #[async_trait::async_trait]
    impl NetworkEffects for CountingMiddleware {
        async fn send_to_peer(
            &self,
            peer_id: DeviceId,
            message: Vec<u8>,
        ) -> Result<(), aura_core::effects::NetworkError> {
            self.count.fetch_add(1, Ordering::Relaxed);
            self.inner.send_to_peer(peer_id, message).await
        }

        async fn recv_from_peer(
            &self,
            peer_id: DeviceId,
        ) -> Result<Vec<u8>, aura_core::effects::NetworkError> {
            self.count.fetch_add(1, Ordering::Relaxed);
            self.inner.recv_from_peer(peer_id).await
        }

        async fn broadcast(
            &self,
            message: Vec<u8>,
        ) -> Result<(), aura_core::effects::NetworkError> {
            self.count.fetch_add(1, Ordering::Relaxed);
            self.inner.broadcast(message).await
        }
    }

    c.bench_function("middleware_depth_1", |b| {
        let base = Arc::new(MockNetworkHandler::new());
        let count = Arc::new(AtomicU64::new(0));
        let wrapped = Arc::new(CountingMiddleware {
            inner: base,
            count: count.clone(),
        });

        b.to_async(&runtime).iter(|| async {
            let _ = wrapped.send_to_peer(DeviceId::new(), vec![0u8; 256]).await;
        });
    });

    c.bench_function("middleware_depth_5", |b| {
        let mut handler: Arc<dyn NetworkEffects> = Arc::new(MockNetworkHandler::new());
        let counts: Vec<Arc<AtomicU64>> = (0..5).map(|_| Arc::new(AtomicU64::new(0))).collect();

        // Wrap 5 times
        for count in &counts {
            handler = Arc::new(CountingMiddleware {
                inner: handler,
                count: count.clone(),
            });
        }

        b.to_async(&runtime).iter(|| async {
            let _ = handler.send_to_peer(DeviceId::new(), vec![0u8; 256]).await;
        });
    });
}

/// Benchmark concurrent handler access
fn bench_concurrent_access(c: &mut Criterion) {
    let runtime = Runtime::new().unwrap();

    c.bench_function("concurrent_reads_10", |b| {
        let handler = Arc::new(InMemoryStorageHandler::new());

        // Pre-populate some data
        runtime.block_on(async {
            for i in 0..100 {
                handler
                    .store(&format!("key_{}", i), vec![0u8; 1024], false)
                    .await
                    .unwrap();
            }
        });

        b.to_async(&runtime).iter(|| async {
            let mut handles = vec![];

            for i in 0..10 {
                let h = handler.clone();
                let handle = tokio::spawn(async move {
                    let _ = h.retrieve(&format!("key_{}", i % 100)).await;
                });
                handles.push(handle);
            }

            for handle in handles {
                let _ = handle.await;
            }
        });
    });

    c.bench_function("concurrent_writes_10", |b| {
        let handler = Arc::new(InMemoryStorageHandler::new());

        b.to_async(&runtime).iter(|| async {
            let mut handles = vec![];

            for i in 0..10 {
                let h = handler.clone();
                let handle = tokio::spawn(async move {
                    let _ = h.store(&format!("key_{}", i), vec![0u8; 1024], false).await;
                });
                handles.push(handle);
            }

            for handle in handles {
                let _ = handle.await;
            }
        });
    });

    c.bench_function("mixed_concurrent_ops", |b| {
        let handler = Arc::new(InMemoryStorageHandler::new());

        b.to_async(&runtime).iter(|| async {
            let mut handles = vec![];

            for i in 0..20 {
                let h = handler.clone();
                let handle = tokio::spawn(async move {
                    if i % 2 == 0 {
                        let _ = h.store(&format!("key_{}", i), vec![0u8; 512], false).await;
                    } else {
                        let _ = h.retrieve(&format!("key_{}", i - 1)).await;
                    }
                });
                handles.push(handle);
            }

            for handle in handles {
                let _ = handle.await;
            }
        });
    });
}

/// Benchmark handler factory performance
fn bench_handler_factory(c: &mut Criterion) {
    let runtime = Runtime::new().unwrap();

    c.bench_function("factory_create_handler", |b| {
        let factory = HandlerFactory::new();

        b.iter(|| {
            let handler = factory.create_network_handler("mock");
            black_box(handler);
        });
    });

    c.bench_function("factory_with_config", |b| {
        let factory = HandlerFactory::new();
        let mut config = HashMap::new();
        config.insert("timeout".to_string(), "30".to_string());
        config.insert("retry_count".to_string(), "3".to_string());

        b.iter(|| {
            let handler = factory.create_network_handler_with_config("mock", &config);
            black_box(handler);
        });
    });
}

/// Benchmark effect executor performance
fn bench_effect_executor(c: &mut Criterion) {
    let runtime = Runtime::new().unwrap();

    c.bench_function("executor_single_effect", |b| {
        let executor = runtime.block_on(async {
            EffectExecutorBuilder::new()
                .with_network_handler(Arc::new(MockNetworkHandler::new()))
                .build()
                .await
        });

        b.to_async(&runtime).iter(|| async {
            let _ = executor
                .execute_network(|handler| async move {
                    handler.send_to_peer(DeviceId::new(), vec![0u8; 256]).await
                })
                .await;
        });
    });

    c.bench_function("executor_batched_effects", |b| {
        let executor = runtime.block_on(async {
            EffectExecutorBuilder::new()
                .with_network_handler(Arc::new(MockNetworkHandler::new()))
                .with_storage_handler(Arc::new(InMemoryStorageHandler::new()))
                .build()
                .await
        });

        b.to_async(&runtime).iter(|| async {
            let mut results = vec![];

            // Execute multiple effects
            let r1 = executor.execute_network(|handler| async move {
                handler.send_to_peer(DeviceId::new(), vec![0u8; 256]).await
            });

            let r2 = executor.execute_storage(|handler| async move {
                handler.store("key", vec![0u8; 512], false).await
            });

            let r3 = executor
                .execute_network(|handler| async move { handler.broadcast(vec![0u8; 128]).await });

            results.push(r1.await);
            results.push(r2.await);
            results.push(r3.await);

            black_box(results);
        });
    });
}

/// Benchmark error handling overhead
fn bench_error_handling(c: &mut Criterion) {
    let runtime = Runtime::new().unwrap();

    struct FailingHandler {
        fail_rate: f32,
    }

    #[async_trait::async_trait]
    impl NetworkEffects for FailingHandler {
        async fn send_to_peer(
            &self,
            _peer_id: DeviceId,
            _message: Vec<u8>,
        ) -> Result<(), aura_core::effects::NetworkError> {
            if rand::random::<f32>() < self.fail_rate {
                Err(aura_core::effects::NetworkError::ConnectionFailed)
            } else {
                Ok(())
            }
        }

        async fn recv_from_peer(
            &self,
            _peer_id: DeviceId,
        ) -> Result<Vec<u8>, aura_core::effects::NetworkError> {
            Ok(vec![])
        }

        async fn broadcast(
            &self,
            _message: Vec<u8>,
        ) -> Result<(), aura_core::effects::NetworkError> {
            Ok(())
        }
    }

    c.bench_function("error_handling_0_percent", |b| {
        let handler = FailingHandler { fail_rate: 0.0 };

        b.to_async(&runtime).iter(|| async {
            let result = handler.send_to_peer(DeviceId::new(), vec![0u8; 256]).await;
            black_box(result);
        });
    });

    c.bench_function("error_handling_50_percent", |b| {
        let handler = FailingHandler { fail_rate: 0.5 };

        b.to_async(&runtime).iter(|| async {
            let result = handler.send_to_peer(DeviceId::new(), vec![0u8; 256]).await;
            black_box(result);
        });
    });
}

criterion_group!(
    benches,
    bench_handler_invocation,
    bench_handler_composition,
    bench_handler_registry,
    bench_middleware_stack,
    bench_concurrent_access,
    bench_handler_factory,
    bench_effect_executor,
    bench_error_handling
);

criterion_main!(benches);
