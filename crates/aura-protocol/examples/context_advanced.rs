//! Advanced context propagation examples with distributed tracing
//!
//! This example demonstrates more advanced patterns including distributed
//! tracing, choreographic protocol integration, and middleware composition.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tracing_subscriber;

use async_trait::async_trait;
use aura_core::{DeviceId, FlowBudget};
use aura_protocol::effects::{
    context::{thread_local, EffectContext, TraceContext},
    contextual::{
        ContextPropagator, ContextualNetworkEffects, ContextualStorageEffects, EffectContextExt,
    },
    migration::{MigrationGuide, MigrationStats, MigrationTool},
    propagation::{BatchContext, ContextGuard, ContextMiddleware},
};
use tracing::{info, span, Level};

/// Example handler with tracing support
struct TracingHandler {
    device_id: DeviceId,
    stats: Arc<tokio::sync::Mutex<MigrationStats>>,
}

#[async_trait]
impl ContextualNetworkEffects for TracingHandler {
    async fn send_to_peer(
        &self,
        ctx: &mut EffectContext,
        peer_id: DeviceId,
        message: Vec<u8>,
    ) -> Result<(), aura_core::effects::NetworkError> {
        // Create child span with trace context
        let _span = span!(
            Level::INFO,
            "send_to_peer",
            trace_id = %ctx.trace_context.trace_id,
            span_id = %ctx.trace_context.span_id,
            peer = %peer_id.0,
        )
        .entered();

        info!("Sending {} bytes to peer", message.len());

        // Record contextual call
        self.stats.lock().await.record_contextual();

        // Charge flow and simulate work
        ctx.charge_flow(10)
            .map_err(|_| aura_core::effects::NetworkError::Timeout)?;
        tokio::time::sleep(Duration::from_millis(20)).await;

        Ok(())
    }

    async fn recv_from_peer(
        &self,
        ctx: &mut EffectContext,
        peer_id: DeviceId,
    ) -> Result<Vec<u8>, aura_core::effects::NetworkError> {
        let _span = span!(
            Level::INFO,
            "recv_from_peer",
            trace_id = %ctx.trace_context.trace_id,
            peer = %peer_id.0,
        )
        .entered();

        self.stats.lock().await.record_contextual();
        ctx.charge_flow(10)
            .map_err(|_| aura_core::effects::NetworkError::Timeout)?;

        Ok(vec![1, 2, 3, 4])
    }

    async fn broadcast(
        &self,
        ctx: &mut EffectContext,
        message: Vec<u8>,
    ) -> Result<(), aura_core::effects::NetworkError> {
        let _span = span!(Level::INFO, "broadcast").entered();

        self.stats.lock().await.record_contextual();
        ctx.charge_flow(50)
            .map_err(|_| aura_core::effects::NetworkError::Timeout)?;

        info!("Broadcasting {} bytes", message.len());
        Ok(())
    }
}

#[async_trait]
impl ContextualStorageEffects for TracingHandler {
    async fn store(
        &self,
        ctx: &mut EffectContext,
        key: &str,
        value: Vec<u8>,
        encrypted: bool,
    ) -> Result<(), aura_core::effects::StorageError> {
        let _span = span!(
            Level::INFO,
            "store",
            key = %key,
            size = value.len(),
            encrypted = encrypted,
        )
        .entered();

        ctx.charge_flow(20)
            .map_err(|_| aura_core::effects::StorageError::QuotaExceeded)?;
        info!("Stored {} bytes at key {}", value.len(), key);
        Ok(())
    }

    async fn retrieve(
        &self,
        ctx: &mut EffectContext,
        key: &str,
    ) -> Result<Option<Vec<u8>>, aura_core::effects::StorageError> {
        ctx.charge_flow(10)
            .map_err(|_| aura_core::effects::StorageError::QuotaExceeded)?;
        Ok(Some(vec![42; 10]))
    }

    async fn delete(
        &self,
        ctx: &mut EffectContext,
        key: &str,
    ) -> Result<(), aura_core::effects::StorageError> {
        ctx.charge_flow(5)
            .map_err(|_| aura_core::effects::StorageError::QuotaExceeded)?;
        Ok(())
    }

    async fn list_keys(
        &self,
        ctx: &mut EffectContext,
        prefix: &str,
    ) -> Result<Vec<String>, aura_core::effects::StorageError> {
        ctx.charge_flow(15)
            .map_err(|_| aura_core::effects::StorageError::QuotaExceeded)?;
        Ok(vec![format!("{}/key1", prefix), format!("{}/key2", prefix)])
    }
}

/// Example 1: Distributed tracing across services
async fn example_distributed_tracing() {
    println!("\n=== Example 1: Distributed Tracing ===");

    let device_id = DeviceId::new();
    let stats = Arc::new(tokio::sync::Mutex::new(MigrationStats::default()));
    let handler = Arc::new(TracingHandler {
        device_id,
        stats: stats.clone(),
    });

    // Create root context with trace info
    let mut root_context = EffectContext::new(device_id)
        .with_flow_budget(FlowBudget::new(500))
        .with_metadata("service", "api-gateway")
        .with_metadata("version", "1.0.0");

    // Simulate receiving a request with W3C trace context
    let trace_headers = HashMap::from([
        (
            "traceparent",
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
        ),
        ("tracestate", "vendor1=value1,vendor2=value2"),
    ]);

    if let Some(trace_parent) = trace_headers.get("traceparent") {
        if let Ok(trace) = TraceContext::from_trace_parent(trace_parent) {
            root_context.trace_context = trace;
            println!("Continuing trace: {}", trace.trace_id);
        }
    }

    // Propagate to downstream service
    let h1 = handler.clone();
    let downstream_task = tokio::spawn(async move {
        let mut ctx = root_context
            .child()
            .with_metadata("service", "backend-service");

        println!(
            "Downstream trace: {} -> {}",
            ctx.trace_context.trace_id, ctx.trace_context.span_id
        );

        // Perform operations
        h1.send_to_peer(&mut ctx, DeviceId::new(), vec![1, 2, 3])
            .await
            .unwrap();
        h1.store(&mut ctx, "trace/result", vec![42], true)
            .await
            .unwrap();

        // Export headers for next hop
        let headers = ctx.trace_context.as_headers();
        println!("Propagating headers: {:?}", headers);
    });

    downstream_task.await.unwrap();
}

/// Example 2: Choreographic protocol with context
async fn example_choreographic_protocol() {
    println!("\n=== Example 2: Choreographic Protocol ===");

    let device_a = DeviceId::new();
    let device_b = DeviceId::new();
    let stats = Arc::new(tokio::sync::Mutex::new(MigrationStats::default()));

    // Protocol participants
    let alice = Arc::new(TracingHandler {
        device_id: device_a,
        stats: stats.clone(),
    });
    let bob = Arc::new(TracingHandler {
        device_id: device_b,
        stats: stats.clone(),
    });

    // Root context for protocol execution
    let protocol_context = EffectContext::new(device_a)
        .with_flow_budget(FlowBudget::new(1000))
        .with_metadata("protocol", "key-exchange")
        .with_metadata("session", "12345");

    // Alice's side of the protocol
    let alice_task = {
        let ctx = protocol_context.child().with_metadata("role", "alice");
        let alice = alice.clone();

        tokio::spawn(async move {
            let mut ctx = ctx;

            // Step 1: Alice sends initial message
            alice
                .send_to_peer(&mut ctx, device_b, vec![1, 2, 3])
                .await
                .unwrap();

            // Step 2: Alice receives response
            let response = alice.recv_from_peer(&mut ctx, device_b).await.unwrap();
            println!("Alice received: {:?}", response);

            // Step 3: Alice broadcasts result
            alice.broadcast(&mut ctx, vec![99]).await.unwrap();
        })
    };

    // Bob's side of the protocol
    let bob_task = {
        let ctx = protocol_context.child().with_metadata("role", "bob");
        let bob = bob.clone();

        tokio::spawn(async move {
            let mut ctx = ctx;

            // Step 1: Bob receives initial message
            let msg = bob.recv_from_peer(&mut ctx, device_a).await.unwrap();
            println!("Bob received: {:?}", msg);

            // Step 2: Bob sends response
            bob.send_to_peer(&mut ctx, device_a, vec![4, 5, 6])
                .await
                .unwrap();

            // Step 3: Bob stores result
            bob.store(&mut ctx, "protocol/result", vec![42], false)
                .await
                .unwrap();
        })
    };

    // Wait for protocol completion
    let _ = tokio::join!(alice_task, bob_task);

    println!("Protocol completed!");
}

/// Example 3: Context middleware composition
async fn example_middleware_composition() {
    println!("\n=== Example 3: Middleware Composition ===");

    let device_id = DeviceId::new();
    let stats = Arc::new(tokio::sync::Mutex::new(MigrationStats::default()));
    let base_handler = TracingHandler {
        device_id,
        stats: stats.clone(),
    };

    // Wrap with context middleware
    let handler = ContextMiddleware::new(base_handler);

    // Create operation context
    let context = EffectContext::new(device_id)
        .with_flow_budget(FlowBudget::new(200))
        .with_metadata("middleware", "example");

    // Use middleware with automatic context propagation
    let result = handler
        .execute(|h| async move {
            // Context is automatically available
            if let Some(mut ctx) = thread_local::current() {
                h.send_to_peer(&mut ctx, DeviceId::new(), vec![])
                    .await
                    .unwrap();
                println!("Middleware operation completed");
            }
        })
        .await;
}

/// Example 4: Batch operations with context
async fn example_batch_operations() {
    println!("\n=== Example 4: Batch Operations ===");

    let mut batch = BatchContext::new();

    // Create contexts for different operations
    for i in 0..5 {
        let context = EffectContext::new(DeviceId::new())
            .with_flow_budget(FlowBudget::new(100))
            .with_metadata("batch_id", i.to_string())
            .with_metadata("operation", format!("task-{}", i));

        batch.add(context);
    }

    // Create operations to execute
    let operations = (0..5)
        .map(|i| async move {
            if let Some(ctx) = aura_protocol::effects::propagation::current_context().await {
                println!("Batch operation {} with context {}", i, ctx.request_id);
                tokio::time::sleep(Duration::from_millis(50)).await;
                format!("Result {}", i)
            } else {
                "No context".to_string()
            }
        })
        .collect();

    // Execute all operations in parallel with their contexts
    let results = batch.execute_all(operations).await;
    println!("Batch results: {:?}", results);
}

/// Example 5: Migration analysis and recommendations
async fn example_migration_analysis() {
    println!("\n=== Example 5: Migration Analysis ===");

    let device_id = DeviceId::new();
    let stats = Arc::new(tokio::sync::Mutex::new(MigrationStats::default()));
    let handler = TracingHandler {
        device_id,
        stats: stats.clone(),
    };

    // Perform some operations
    let mut ctx = EffectContext::new(device_id).with_flow_budget(FlowBudget::new(1000));

    for _ in 0..10 {
        handler
            .send_to_peer(&mut ctx, DeviceId::new(), vec![])
            .await
            .unwrap();
    }

    // Simulate some non-contextual calls
    {
        let mut stats = stats.lock().await;
        for _ in 0..15 {
            stats.record_non_contextual();
        }
        stats.record_error();
        stats.record_error();
    }

    // Analyze migration progress
    let final_stats = stats.lock().await;
    println!("\nMigration Statistics:");
    println!("  Total calls: {}", final_stats.total_calls);
    println!("  Contextual calls: {}", final_stats.contextual_calls);
    println!(
        "  Non-contextual calls: {}",
        final_stats.non_contextual_calls
    );
    println!("  Migration errors: {}", final_stats.migration_errors);
    println!("  Completion: {:.1}%", final_stats.completion_percentage());

    // Get recommendations
    let guide = MigrationGuide::analyze(&final_stats);
    println!("\nMigration Recommendations:");
    for (i, rec) in guide.recommendations().iter().enumerate() {
        println!("  {}. {}", i + 1, rec);
    }
}

/// Example 6: Context guards for synchronous code
fn example_sync_context_guard() {
    println!("\n=== Example 6: Context Guards (Sync) ===");

    let context = EffectContext::new(DeviceId::new()).with_metadata("mode", "synchronous");

    // No context available initially
    assert!(thread_local::current().is_none());

    {
        // Enter context scope
        let _guard = ContextGuard::enter(context.clone());

        // Context is now available
        let current = thread_local::current().unwrap();
        println!("In guard scope: {}", current.request_id);
        assert_eq!(
            current.metadata.get("mode"),
            Some(&"synchronous".to_string())
        );

        // Nested guard
        {
            let nested_ctx = current.child().with_metadata("nested", "true");
            let _nested_guard = ContextGuard::enter(nested_ctx);

            let nested_current = thread_local::current().unwrap();
            println!("In nested scope: {}", nested_current.request_id);
            assert_eq!(
                nested_current.metadata.get("nested"),
                Some(&"true".to_string())
            );
        }

        // Back to parent context
        let current = thread_local::current().unwrap();
        assert_eq!(
            current.metadata.get("mode"),
            Some(&"synchronous".to_string())
        );
    }

    // Context cleared after guard drops
    assert!(thread_local::current().is_none());
}

/// Example 7: Migration tool usage
async fn example_migration_tool() {
    println!("\n=== Example 7: Migration Tool ===");

    let device_id = DeviceId::new();
    let tool = MigrationTool::new(device_id);

    // Old handler that needs migration
    struct LegacyHandler;

    #[async_trait]
    impl aura_core::effects::NetworkEffects for LegacyHandler {
        async fn send_to_peer(
            &self,
            peer_id: DeviceId,
            message: Vec<u8>,
        ) -> Result<(), aura_core::effects::NetworkError> {
            println!("Legacy send to {}", peer_id.0);
            Ok(())
        }

        async fn recv_from_peer(
            &self,
            peer_id: DeviceId,
        ) -> Result<Vec<u8>, aura_core::effects::NetworkError> {
            Ok(vec![99])
        }

        async fn broadcast(
            &self,
            message: Vec<u8>,
        ) -> Result<(), aura_core::effects::NetworkError> {
            Ok(())
        }
    }

    // Wrap legacy handler
    let legacy = LegacyHandler;
    let migrated = tool.wrap(legacy);

    // Run operations with automatic context
    let result = tool
        .run_with_context(async {
            let mut ctx = tool.create_context().with_flow_budget(FlowBudget::new(100));

            migrated
                .send_to_peer(&mut ctx, DeviceId::new(), vec![1, 2, 3])
                .await?;

            println!("Migration context metadata: {:?}", ctx.metadata);
            Ok::<_, aura_core::effects::NetworkError>(())
        })
        .await;

    println!("Migration result: {:?}", result.is_ok());
}

#[tokio::main]
async fn main() {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    println!("Advanced Context Propagation Examples\n");

    example_distributed_tracing().await;
    example_choreographic_protocol().await;
    example_middleware_composition().await;
    example_batch_operations().await;
    example_migration_analysis().await;
    example_sync_context_guard();
    example_migration_tool().await;

    println!("\nAll examples completed!");
}
