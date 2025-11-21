//! Performance benchmarks for context propagation
//!
//! These benchmarks measure:
//! - Context creation and cloning overhead
//! - Task-local storage performance
//! - Context propagation through async calls
//! - Tracing span overhead

use criterion::{black_box, criterion_group, criterion_main, BatchSize, Criterion};
use tokio::runtime::Runtime;

use aura_core::{FlowBudget, identifiers::DeviceId};
use aura_effects::handlers::MockNetworkHandler;
use aura_protocol::effects::{
    context::{EffectContext, TraceContext, WithContext},
    contextual::{ContextAdapter, ContextualNetworkEffects},
    migration::{MigrationAdapter, MigrationTool},
    propagation::{
        current_context, spawn_with_context, with_context, BatchContext, ContextGuard,
        PropagateContext,
    },
};
use std::sync::Arc;
use std::time::{Duration, Instant};
use uuid::Uuid;

/// Benchmark context creation and manipulation
fn bench_context_operations(c: &mut Criterion) {
    c.bench_function("context_creation", |b| {
        b.iter(|| {
            let context = EffectContext::new(DeviceId::new());
            black_box(context);
        });
    });

    c.bench_function("context_with_metadata", |b| {
        b.iter(|| {
            let context = EffectContext::new(DeviceId::new())
                .with_flow_budget(FlowBudget::new(1000))
                .with_deadline(Instant::now() + Duration::from_secs(30))
                .with_metadata("key1", "value1")
                .with_metadata("key2", "value2")
                .with_metadata("key3", "value3");
            black_box(context);
        });
    });

    c.bench_function("context_cloning", |b| {
        let context = EffectContext::new(DeviceId::new())
            .with_flow_budget(FlowBudget::new(1000))
            .with_metadata("test", "value");

        b.iter(|| {
            let cloned = context.clone();
            black_box(cloned);
        });
    });

    c.bench_function("context_child_creation", |b| {
        let parent = EffectContext::new(DeviceId::new()).with_flow_budget(FlowBudget::new(1000));

        b.iter(|| {
            let child = parent.child();
            black_box(child);
        });
    });

    c.bench_function("context_hierarchy_depth", |b| {
        b.iter_batched(
            || EffectContext::new(DeviceId::new()),
            |mut context| {
                // Create deep hierarchy
                for _ in 0..10 {
                    context = context.child();
                }
                let root = context.root();
                black_box(root);
            },
            BatchSize::SmallInput,
        );
    });
}

/// Benchmark trace context operations
fn bench_trace_context(c: &mut Criterion) {
    c.bench_function("trace_context_creation", |b| {
        b.iter(|| {
            let trace = TraceContext::new();
            black_box(trace);
        });
    });

    c.bench_function("trace_context_from_header", |b| {
        let header = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";
        b.iter(|| {
            let trace = TraceContext::from_trace_parent(header).unwrap();
            black_box(trace);
        });
    });

    c.bench_function("trace_context_child", |b| {
        let parent = TraceContext::new();
        b.iter(|| {
            let child = parent.child();
            black_box(child);
        });
    });

    c.bench_function("trace_headers_export", |b| {
        let trace = TraceContext::new();
        b.iter(|| {
            let headers = trace.as_headers();
            black_box(headers);
        });
    });
}

/// Benchmark context propagation through async operations
fn bench_async_propagation(c: &mut Criterion) {
    let runtime = Runtime::new().unwrap();

    c.bench_function("task_local_set_get", |b| {
        b.to_async(&runtime).iter(|| async {
            let context = EffectContext::new(DeviceId::new());
            with_context(context, async {
                let current = current_context().await;
                black_box(current);
            })
            .await;
        });
    });

    c.bench_function("nested_context_propagation", |b| {
        b.to_async(&runtime).iter(|| async {
            let root = EffectContext::new(DeviceId::new());

            with_context(root, async {
                let ctx1 = current_context().await.unwrap();

                with_context(ctx1.child(), async {
                    let ctx2 = current_context().await.unwrap();

                    with_context(ctx2.child(), async {
                        let ctx3 = current_context().await.unwrap();
                        black_box(ctx3);
                    })
                    .await;
                })
                .await;
            })
            .await;
        });
    });

    c.bench_function("spawn_with_context_overhead", |b| {
        b.to_async(&runtime).iter(|| async {
            let context = EffectContext::new(DeviceId::new());
            let handle = spawn_with_context(context, async {
                let _ = current_context().await;
                42
            });
            let result = handle.await.unwrap();
            black_box(result);
        });
    });

    c.bench_function("future_propagation", |b| {
        b.to_async(&runtime).iter(|| async {
            let context = EffectContext::new(DeviceId::new());

            let future = async {
                let ctx = current_context().await;
                black_box(ctx);
            };

            future.with_propagated_context(context).await;
        });
    });
}

/// Benchmark context guards for synchronous code
fn bench_context_guards(c: &mut Criterion) {
    c.bench_function("context_guard_enter_exit", |b| {
        let context = EffectContext::new(DeviceId::new());

        b.iter(|| {
            let _guard = ContextGuard::enter(context.clone());
            // Guard automatically drops
        });
    });

    c.bench_function("nested_guards", |b| {
        let ctx1 = EffectContext::new(DeviceId::new());
        let ctx2 = ctx1.child();
        let ctx3 = ctx2.child();

        b.iter(|| {
            let _g1 = ContextGuard::enter(ctx1.clone());
            {
                let _g2 = ContextGuard::enter(ctx2.clone());
                {
                    let _g3 = ContextGuard::enter(ctx3.clone());
                    // All guards drop in reverse order
                }
            }
        });
    });
}

/// Benchmark batch context operations
fn bench_batch_operations(c: &mut Criterion) {
    let runtime = Runtime::new().unwrap();

    c.bench_function("batch_context_10_operations", |b| {
        b.to_async(&runtime).iter(|| async {
            let mut batch = BatchContext::new();

            // Add 10 contexts
            for i in 0..10 {
                let context =
                    EffectContext::new(DeviceId::new()).with_metadata("index", i.to_string());
                batch.add(context);
            }

            // Create 10 operations
            let operations: Vec<_> = (0..10).map(|i| async move { i }).collect();

            let results = batch.execute_all(operations).await;
            black_box(results);
        });
    });

    c.bench_function("batch_context_100_operations", |b| {
        b.to_async(&runtime).iter(|| async {
            let mut batch = BatchContext::new();

            // Add 100 contexts
            for i in 0..100 {
                let context =
                    EffectContext::new(DeviceId::new()).with_metadata("index", i.to_string());
                batch.add(context);
            }

            // Create 100 operations
            let operations: Vec<_> = (0..100).map(|i| async move { i }).collect();

            let results = batch.execute_all(operations).await;
            black_box(results);
        });
    });
}

/// Benchmark migration overhead
fn bench_migration(c: &mut Criterion) {
    let runtime = Runtime::new().unwrap();

    c.bench_function("migration_adapter_overhead", |b| {
        let device_id = DeviceId::new();
        let handler = MockNetworkHandler::new();
        let adapter = MigrationAdapter::new(handler, device_id);

        b.to_async(&runtime).iter(|| async {
            let mut context = EffectContext::new(device_id).with_flow_budget(FlowBudget::new(1000));

            let _ = adapter
                .send_to_peer(&mut context, DeviceId::new(), vec![])
                .await;
        });
    });

    c.bench_function("migration_tool_wrapper", |b| {
        let device_id = DeviceId::new();
        let tool = MigrationTool::new(device_id);

        b.iter(|| {
            let handler = MockNetworkHandler::new();
            let wrapped = tool.wrap(handler);
            black_box(wrapped);
        });
    });

    c.bench_function("migration_context_creation", |b| {
        let device_id = DeviceId::new();
        let tool = MigrationTool::new(device_id);

        b.iter(|| {
            let context = tool.create_context();
            black_box(context);
        });
    });
}

/// Benchmark flow budget operations
fn bench_flow_budget(c: &mut Criterion) {
    c.bench_function("flow_budget_charging", |b| {
        b.iter_batched(
            || EffectContext::new(DeviceId::new()).with_flow_budget(FlowBudget::new(10000)),
            |mut context| {
                // Charge budget 100 times
                for _ in 0..100 {
                    let _ = context.charge_flow(10);
                }
                black_box(context);
            },
            BatchSize::SmallInput,
        );
    });

    c.bench_function("deadline_checking", |b| {
        let context = EffectContext::new(DeviceId::new())
            .with_deadline(Instant::now() + Duration::from_secs(60));

        b.iter(|| {
            let exceeded = context.is_deadline_exceeded();
            let remaining = context.time_until_deadline();
            black_box((exceeded, remaining));
        });
    });
}

/// Benchmark contextual effect trait overhead
fn bench_contextual_traits(c: &mut Criterion) {
    let runtime = Runtime::new().unwrap();

    // Mock contextual handler
    struct MockContextualHandler;

    #[async_trait::async_trait]
    impl ContextualNetworkEffects for MockContextualHandler {
        async fn send_to_peer(
            &self,
            ctx: &mut EffectContext,
            _peer_id: DeviceId,
            _message: Vec<u8>,
        ) -> Result<(), aura_core::effects::NetworkError> {
            ctx.charge_flow(10).ok();
            Ok(())
        }

        async fn recv_from_peer(
            &self,
            ctx: &mut EffectContext,
            _peer_id: DeviceId,
        ) -> Result<Vec<u8>, aura_core::effects::NetworkError> {
            ctx.charge_flow(10).ok();
            Ok(vec![])
        }

        async fn broadcast(
            &self,
            ctx: &mut EffectContext,
            _message: Vec<u8>,
        ) -> Result<(), aura_core::effects::NetworkError> {
            ctx.charge_flow(50).ok();
            Ok(())
        }
    }

    let handler = MockContextualHandler;

    c.bench_function("contextual_send_overhead", |b| {
        b.to_async(&runtime).iter_batched(
            || EffectContext::new(DeviceId::new()).with_flow_budget(FlowBudget::new(1000)),
            |mut context| async {
                let _ = handler
                    .send_to_peer(&mut context, DeviceId::new(), vec![])
                    .await;
                black_box(context);
            },
            BatchSize::SmallInput,
        );
    });
}

criterion_group!(
    benches,
    bench_context_operations,
    bench_trace_context,
    bench_async_propagation,
    bench_context_guards,
    bench_batch_operations,
    bench_migration,
    bench_flow_budget,
    bench_contextual_traits
);

criterion_main!(benches);
