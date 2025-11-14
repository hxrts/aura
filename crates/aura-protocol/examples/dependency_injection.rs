//! Example demonstrating dependency injection patterns in AuraEffectSystem
//!
//! This example shows how to:
//! - Create and configure an effect container
//! - Register handlers with different scopes
//! - Use test fixtures for common configurations
//! - Override handlers for testing
//! - Use scoped containers for test isolation

use aura_core::{AuraResult, DeviceId};
use aura_protocol::effects::{
    AuraEffectSystemBuilder,
    container::{EffectContainer, TestFixture, ScopedContainer},
};
use aura_protocol::ExecutionMode;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::time::{sleep, Duration};
use tracing::info;

/// Custom crypto handler for demonstration
#[derive(Clone)]
struct CustomCryptoHandler {
    name: String,
    call_count: Arc<AtomicU64>,
}

impl CustomCryptoHandler {
    fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            call_count: Arc::new(AtomicU64::new(0)),
        }
    }

    fn get_call_count(&self) -> u64 {
        self.call_count.load(Ordering::Relaxed)
    }
}

// Note: In a real implementation, we'd implement CryptoEffects trait
// For this example, we're just demonstrating the pattern

#[tokio::main]
async fn main() -> AuraResult<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_target(false)
        .with_timestamp(true)
        .init();

    println!("=== Aura Effect System Dependency Injection Example ===\n");

    // Example 1: Basic Container Usage
    example_basic_container().await?;
    
    // Example 2: Test Fixtures
    example_test_fixtures().await?;
    
    // Example 3: Scoped Containers
    example_scoped_containers().await?;
    
    // Example 4: Handler Override Pattern
    example_handler_override().await?;

    println!("\n=== All examples completed successfully ===");
    Ok(())
}

async fn example_basic_container() -> AuraResult<()> {
    println!("\n--- Example 1: Basic Container Usage ---\n");
    
    // Create a container
    let container = Arc::new(EffectContainer::new());
    
    // Register handlers with different scopes
    
    // Singleton - one instance for entire application
    let singleton_counter = Arc::new(AtomicU64::new(0));
    let singleton_counter_clone = singleton_counter.clone();
    container.register_singleton(move || {
        singleton_counter_clone.fetch_add(1, Ordering::Relaxed);
        CustomCryptoHandler::new("singleton_crypto")
    }).await;
    
    // Transient - new instance for each resolution
    let transient_counter = Arc::new(AtomicU64::new(0));
    let transient_counter_clone = transient_counter.clone();
    container.register_transient(move || {
        let count = transient_counter_clone.fetch_add(1, Ordering::Relaxed);
        CustomCryptoHandler::new(format!("transient_crypto_{}", count))
    }).await;
    
    // Resolve handlers multiple times
    println!("Resolving singleton handler twice:");
    let handler1 = container.resolve::<CustomCryptoHandler>().await?;
    let handler2 = container.resolve::<CustomCryptoHandler>().await?;
    println!("  First resolution: {}", handler1.name);
    println!("  Second resolution: {}", handler2.name);
    println!("  Singleton created {} time(s)", singleton_counter.load(Ordering::Relaxed));
    
    // Note: In this example, we're only registering one type, so transient won't be resolved
    // In a real scenario, you'd register different types with different scopes
    
    Ok(())
}

async fn example_test_fixtures() -> AuraResult<()> {
    println!("\n--- Example 2: Test Fixtures ---\n");
    
    // Create a test fixture with mock handlers
    let fixture = TestFixture::new();
    let container = Arc::new(fixture.with_mocks().await);
    
    println!("Created test fixture with mock handlers");
    
    // Build an effect system using the fixture container
    let device_id = DeviceId::new();
    let effect_system = AuraEffectSystemBuilder::new()
        .with_device_id(device_id)
        .with_execution_mode(ExecutionMode::Testing)
        .with_container(container.clone())
        .build()
        .await?;
    
    println!("Built effect system with fixture container");
    println!("  Device ID: {:?}", effect_system.device_id());
    println!("  Lifecycle state: {:?}", effect_system.lifecycle_state());
    
    // The effect system now uses handlers from the container
    
    Ok(())
}

async fn example_scoped_containers() -> AuraResult<()> {
    println!("\n--- Example 3: Scoped Containers ---\n");
    
    let container = Arc::new(EffectContainer::new());
    
    // Test 1 with its own scope
    {
        println!("Test 1 - Creating scoped container");
        let scoped = ScopedContainer::new(container.clone()).await;
        
        // Register a handler specific to this test
        let test1_handler = CustomCryptoHandler::new("test1_handler");
        scoped.inner().register_instance(test1_handler).await;
        
        // Resolve within scope
        let handler = scoped.inner().resolve::<CustomCryptoHandler>().await?;
        println!("  Resolved handler: {}", handler.name);
    }
    // Scope automatically cleaned up when dropped
    
    // Test 2 with its own isolated scope
    {
        println!("\nTest 2 - Creating another scoped container");
        let scoped = ScopedContainer::new(container.clone()).await;
        
        // Register a different handler for this test
        let test2_handler = CustomCryptoHandler::new("test2_handler");
        scoped.inner().register_instance(test2_handler).await;
        
        // Resolve within scope
        let handler = scoped.inner().resolve::<CustomCryptoHandler>().await?;
        println!("  Resolved handler: {}", handler.name);
    }
    
    // Wait a bit for cleanup tasks
    sleep(Duration::from_millis(100)).await;
    
    println!("\nBoth test scopes have been cleaned up");
    
    Ok(())
}

async fn example_handler_override() -> AuraResult<()> {
    println!("\n--- Example 4: Handler Override Pattern ---\n");
    
    // Start with a base container
    let container = Arc::new(EffectContainer::new());
    
    // Register default handler
    container.register_singleton(|| {
        CustomCryptoHandler::new("default_crypto")
    }).await;
    
    println!("Registered default handler");
    
    // For testing, override with a different implementation
    let test_handler = CustomCryptoHandler::new("test_override_crypto");
    container.register_instance(test_handler.clone()).await;
    
    println!("Overrode with test handler");
    
    // Resolve - should get the overridden handler
    let resolved = container.resolve::<CustomCryptoHandler>().await?;
    println!("Resolved handler: {}", resolved.name);
    
    // Demonstrate usage tracking
    for i in 0..3 {
        test_handler.call_count.fetch_add(1, Ordering::Relaxed);
        println!("  Simulated call {}", i + 1);
    }
    
    println!("Total calls: {}", test_handler.get_call_count());
    
    Ok(())
}

// Example of creating a custom test fixture
async fn create_custom_fixture() -> Arc<EffectContainer> {
    let fixture = TestFixture::new();
    
    // Use the custom setup method
    Arc::new(fixture.with_custom(|container| {
        Box::pin(async move {
            // Register specific handlers for your test scenario
            container.register_singleton(|| {
                CustomCryptoHandler::new("custom_fixture_crypto")
            }).await;
            
            // Add more custom registrations as needed
            info!("Custom fixture configured");
        })
    }).await)
}