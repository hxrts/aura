//! Examples demonstrating context propagation patterns
//!
//! This example shows how to use the new context-aware effect system
//! with proper context propagation through async operations.

use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use aura_core::{DeviceId, FlowBudget};
use aura_protocol::effects::{
    context::{EffectContext, WithContext},
    contextual::{ContextualEffects, ContextualNetworkEffects, ContextualStorageEffects},
    migration::{MigrationAdapter, MigrationTool},
    propagation::{current_context, spawn_with_context, with_context, PropagateContext},
};

/// Example contextual effect handler
struct ExampleEffectHandler {
    device_id: DeviceId,
}

#[async_trait]
impl ContextualNetworkEffects for ExampleEffectHandler {
    async fn send_to_peer(
        &self,
        ctx: &mut EffectContext,
        peer_id: DeviceId,
        message: Vec<u8>,
    ) -> Result<(), aura_core::effects::NetworkError> {
        println!("[{}] Sending message to peer {}", ctx.request_id, peer_id.0);

        // Charge flow budget
        ctx.charge_flow(10)
            .map_err(|_| aura_core::effects::NetworkError::Timeout)?;

        // Simulate network delay
        tokio::time::sleep(Duration::from_millis(50)).await;

        println!(
            "[{}] Message sent, flow budget remaining: {}",
            ctx.request_id,
            ctx.flow_budget.remaining()
        );

        Ok(())
    }

    async fn recv_from_peer(
        &self,
        ctx: &mut EffectContext,
        peer_id: DeviceId,
    ) -> Result<Vec<u8>, aura_core::effects::NetworkError> {
        println!("[{}] Receiving from peer {}", ctx.request_id, peer_id.0);
        ctx.charge_flow(10)
            .map_err(|_| aura_core::effects::NetworkError::Timeout)?;
        Ok(vec![1, 2, 3, 4])
    }

    async fn broadcast(
        &self,
        ctx: &mut EffectContext,
        message: Vec<u8>,
    ) -> Result<(), aura_core::effects::NetworkError> {
        println!("[{}] Broadcasting message", ctx.request_id);
        ctx.charge_flow(50)
            .map_err(|_| aura_core::effects::NetworkError::Timeout)?;
        Ok(())
    }
}

/// Example 1: Basic context propagation
async fn example_basic_propagation() {
    println!("\n=== Example 1: Basic Context Propagation ===");

    let device_id = DeviceId::new();
    let handler = ExampleEffectHandler { device_id };

    // Create a root context with flow budget
    let mut context = EffectContext::new(device_id)
        .with_flow_budget(FlowBudget::new(100))
        .with_metadata("operation", "example1");

    // Use context directly
    let peer_id = DeviceId::new();
    handler
        .send_to_peer(&mut context, peer_id, vec![1, 2, 3])
        .await
        .unwrap();

    println!(
        "Flow budget after operation: {}",
        context.flow_budget.remaining()
    );
}

/// Example 2: Automatic propagation through async calls
async fn example_automatic_propagation() {
    println!("\n=== Example 2: Automatic Propagation ===");

    let device_id = DeviceId::new();
    let handler = Arc::new(ExampleEffectHandler { device_id });

    let context = EffectContext::new(device_id).with_flow_budget(FlowBudget::new(200));

    // Run with context - it will be available in nested calls
    with_context(context.clone(), async {
        // Context is automatically available here
        let current = current_context().await.unwrap();
        println!("Current request ID: {}", current.request_id);

        // Spawn nested operations that inherit context
        let h1 = handler.clone();
        let task1 = spawn_with_context(current.clone(), async move {
            let mut ctx = current_context().await.unwrap();
            h1.send_to_peer(&mut ctx, DeviceId::new(), vec![]).await
        });

        let h2 = handler.clone();
        let task2 = spawn_with_context(current.clone(), async move {
            let mut ctx = current_context().await.unwrap();
            h2.broadcast(&mut ctx, vec![]).await
        });

        // Wait for both tasks
        let _ = tokio::join!(task1, task2);
    })
    .await;
}

/// Example 3: Context hierarchy with parent-child relationships
async fn example_context_hierarchy() {
    println!("\n=== Example 3: Context Hierarchy ===");

    let device_id = DeviceId::new();
    let handler = ExampleEffectHandler { device_id };

    // Create root context
    let root_context = EffectContext::new(device_id)
        .with_flow_budget(FlowBudget::new(300))
        .with_metadata("operation", "root");

    println!("Root context ID: {}", root_context.request_id);

    // Create child context for sub-operation
    let mut child_context = root_context.child().with_metadata("operation", "child");

    println!(
        "Child context ID: {} (parent: {})",
        child_context.request_id, root_context.request_id
    );

    // Operations on child context
    handler
        .send_to_peer(&mut child_context, DeviceId::new(), vec![])
        .await
        .unwrap();

    // Create grandchild context
    let mut grandchild = child_context
        .child()
        .with_metadata("operation", "grandchild");

    handler
        .send_to_peer(&mut grandchild, DeviceId::new(), vec![])
        .await
        .unwrap();

    // Show hierarchy
    println!("Grandchild -> Root: {}", grandchild.root().request_id);
}

/// Example 4: Deadline enforcement
async fn example_deadline_enforcement() {
    println!("\n=== Example 4: Deadline Enforcement ===");

    let device_id = DeviceId::new();
    let handler = ExampleEffectHandler { device_id };

    // Create context with deadline
    let deadline = Instant::now() + Duration::from_millis(100);
    let mut context = EffectContext::new(device_id)
        .with_flow_budget(FlowBudget::new(100))
        .with_deadline(deadline);

    println!("Operation deadline: {:?}", context.time_until_deadline());

    // Perform operations
    handler
        .send_to_peer(&mut context, DeviceId::new(), vec![])
        .await
        .unwrap();

    // Simulate some work
    tokio::time::sleep(Duration::from_millis(150)).await;

    // Check deadline
    if context.is_deadline_exceeded() {
        println!("Deadline exceeded! Aborting remaining operations.");
    }
}

/// Example 5: Migration from old to new API
async fn example_migration() {
    println!("\n=== Example 5: Migration Patterns ===");

    // Old-style handler (implements non-contextual NetworkEffects)
    struct OldHandler;

    #[async_trait]
    impl aura_core::effects::NetworkEffects for OldHandler {
        async fn send_to_peer(
            &self,
            peer_id: DeviceId,
            message: Vec<u8>,
        ) -> Result<(), aura_core::effects::NetworkError> {
            println!("Old handler: sending to {}", peer_id.0);
            Ok(())
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

    let device_id = DeviceId::new();
    let old_handler = OldHandler;

    // Use migration adapter to add context support
    let adapter = MigrationAdapter::new(old_handler, device_id);

    let mut context = EffectContext::new(device_id).with_flow_budget(FlowBudget::new(100));

    // Now we can use the old handler with context!
    adapter
        .send_to_peer(&mut context, DeviceId::new(), vec![])
        .await
        .unwrap();

    println!(
        "Migration successful! Flow budget: {}",
        context.flow_budget.remaining()
    );
}

/// Example 6: Future extension for propagation
async fn example_future_extension() {
    println!("\n=== Example 6: Future Extension ===");

    let device_id = DeviceId::new();
    let context = EffectContext::new(device_id).with_metadata("example", "6");

    // Any future can be extended with context propagation
    let future = async {
        // This will have access to the context
        if let Some(ctx) = current_context().await {
            println!("Future has context: {}", ctx.request_id);
            println!("Metadata: {:?}", ctx.metadata);
        }
        42
    };

    // Propagate context through the future
    let result = future.with_propagated_context(context).await;
    println!("Future result: {}", result);
}

/// Example 7: Error handling with context
async fn example_error_handling() {
    println!("\n=== Example 7: Error Handling ===");

    let device_id = DeviceId::new();
    let handler = ExampleEffectHandler { device_id };

    // Create context with limited budget
    let mut context = EffectContext::new(device_id).with_flow_budget(FlowBudget::new(20));

    // Try to perform operations that exceed budget
    let result1 = handler
        .send_to_peer(&mut context, DeviceId::new(), vec![])
        .await;
    println!(
        "First operation: {:?}, remaining budget: {}",
        result1.is_ok(),
        context.flow_budget.remaining()
    );

    let result2 = handler.broadcast(&mut context, vec![]).await;
    println!(
        "Broadcast operation: {:?} (requires 50, have {})",
        result2.is_err(),
        context.flow_budget.remaining()
    );
}

#[tokio::main]
async fn main() {
    println!("Context Propagation Examples\n");

    example_basic_propagation().await;
    example_automatic_propagation().await;
    example_context_hierarchy().await;
    example_deadline_enforcement().await;
    example_migration().await;
    example_future_extension().await;
    example_error_handling().await;

    println!("\nAll examples completed!");
}
