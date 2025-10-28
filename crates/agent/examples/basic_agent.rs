//! Basic Agent Usage Example
//!
//! This example demonstrates how to use the refactored DeviceAgent
//! with clean service delegation patterns.

use aura_agent::{
    core::DeviceAgent,
    services::{AccountService, ConfigurationManager, IdentityService, ServiceRegistry},
    types::IdentityConfig,
    ContextCapsule, Result,
};
use aura_types::{AccountId, DeviceId};
use std::sync::Arc;
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::init();

    println!("=== Aura Agent: Basic Usage Example ===\n");

    // 1. Create device and account identifiers
    let device_id = DeviceId(Uuid::new_v4());
    let account_id = AccountId(Uuid::new_v4());

    println!("Created device: {}", device_id.0);
    println!("Created account: {}\n", account_id.0);

    // 2. Create agent configuration
    let config = IdentityConfig {
        device_id,
        account_id,
        // Add other required configuration fields
    };

    // 3. Create agent with default services (simple case)
    println!("Creating agent with default services...");
    let agent = DeviceAgent::with_default_services(device_id, account_id, config).await?;
    println!("[OK] Agent created successfully\n");

    // 4. Demonstrate 3-line service delegation methods

    // Identity derivation (was 50+ lines, now 3 lines)
    println!("Deriving simple identity...");
    let identity = agent
        .derive_simple_identity("my-app", "user-session")
        .await?;
    println!("[OK] Derived identity for app: {}", identity.capsule.app_id);

    // Account bootstrap (clean service delegation)
    println!("\nBootstrapping account...");
    let initial_devices = vec![device_id];
    agent.bootstrap_account(initial_devices, 1).await?;
    println!("[OK] Account bootstrapped successfully");

    // Session statistics (clean delegation)
    println!("\nGetting session statistics...");
    let stats = agent.get_session_stats().await?;
    println!("[OK] Retrieved session statistics");

    println!("\n=== Example completed successfully! ===");
    println!("\nKey benefits of the refactored architecture:");
    println!("• 3-line methods with clear business logic");
    println!("• Service delegation eliminates complexity");
    println!("• Clean separation of concerns");
    println!("• Testable service boundaries");
    println!("• No layer violations");

    Ok(())
}

/// Example of creating agent with custom services
#[allow(dead_code)]
async fn example_with_custom_services() -> Result<()> {
    let device_id = DeviceId(Uuid::new_v4());
    let account_id = AccountId(Uuid::new_v4());

    // Create custom service registry
    let services = Arc::new(
        ServiceRegistry::new()
            .with_identity_service(Arc::new(IdentityService::new()))
            .with_account_service(Arc::new(AccountService::new()))
            .with_config_manager(Arc::new(ConfigurationManager::new())),
    );

    let config = IdentityConfig {
        device_id,
        account_id,
        // Add other required fields
    };

    // Create agent with custom services
    let agent = DeviceAgent::new(device_id, account_id, config, services);

    println!("Agent created with custom service configuration");

    Ok(())
}
