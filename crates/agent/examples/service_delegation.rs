//! Service Delegation Pattern Example
//!
//! This example demonstrates how the refactored DeviceAgent uses
//! service delegation to achieve 3-line methods and clean separation of concerns.

use aura_agent::{
    core::DeviceAgent,
    services::{
        AccountService, ConfigurationManager, IdentityService, ServiceError, ServiceRegistry,
        ServiceResult,
    },
    types::IdentityConfig,
    AgentError, ContextCapsule, Result,
};
use aura_types::{AccountId, DeviceId};
use std::sync::Arc;
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::init();

    println!("=== Service Delegation Pattern Demo ===\n");

    demonstrate_before_and_after().await?;
    println!();
    demonstrate_service_composition().await?;
    println!();
    demonstrate_error_handling().await?;

    Ok(())
}

/// Show the difference between monolithic and service-delegated approaches
async fn demonstrate_before_and_after() -> Result<()> {
    println!("--- Before vs After: Service Delegation ---");

    let device_id = DeviceId(Uuid::new_v4());
    let account_id = AccountId(Uuid::new_v4());
    let config = IdentityConfig {
        device_id,
        account_id,
        // Add required fields
    };

    let agent = DeviceAgent::with_default_services(device_id, account_id, config).await?;

    println!("BEFORE (Old DeviceAgent):");
    println!("• derive_context_identity_threshold(): 50+ lines");
    println!("  - Manual context byte construction");
    println!("  - Direct session runtime interaction with 11+ parameters");
    println!("  - Low-level key material handling");
    println!("  - Manual error conversion and validation");
    println!("  - Complex fingerprint computation");
    println!("  - Layer violations mixing high/low-level operations");

    println!("\nAFTER (Refactored DeviceAgent):");
    println!("• derive_context_identity_threshold(): 3 lines");

    // Demonstrate the actual 3-line method
    let capsule = ContextCapsule {
        app_id: "demo-app".to_string(),
        context_label: "user-session".to_string(),
        policy_hint: None,
        transport_hint: None,
        ttl: Some(24 * 3600),
        issued_at: 1234567890,
    };

    println!("  Line 1: Get identity service from registry");
    println!("  Line 2: Call service.derive_threshold_identity()");
    println!("  Line 3: Convert error and return result");

    let identity = agent
        .derive_context_identity_threshold(&capsule, vec![device_id], 1, false)
        .await?;

    println!(
        "[OK] Identity derived successfully: {}",
        identity.capsule.app_id
    );

    println!("\nBenefits of Service Delegation:");
    println!("• Clean separation of concerns");
    println!("• Testable service boundaries");
    println!("• No layer violations");
    println!("• Minimal cognitive load");
    println!("• Easy to understand and maintain");

    Ok(())
}

/// Demonstrate how services can be composed and customized
async fn demonstrate_service_composition() -> Result<()> {
    println!("--- Service Composition ---");

    // Create custom services
    let identity_service = Arc::new(IdentityService::new());
    let account_service = Arc::new(AccountService::new());
    let config_manager = Arc::new(ConfigurationManager::new());

    // Compose services in registry
    let services = Arc::new(
        ServiceRegistry::new()
            .with_identity_service(identity_service.clone())
            .with_account_service(account_service.clone())
            .with_config_manager(config_manager.clone()),
    );

    println!("[OK] Created custom service composition");

    // Services can be used independently
    println!("\nDirect service usage:");

    let derived = identity_service
        .derive_simple_identity("app", "context")
        .await?;
    println!("• IdentityService.derive_simple_identity(): [OK]");

    // Or through the agent (service delegation)
    let device_id = DeviceId(Uuid::new_v4());
    let account_id = AccountId(Uuid::new_v4());
    let config = IdentityConfig {
        device_id,
        account_id,
    };

    let agent = DeviceAgent::new(device_id, account_id, config, services);
    println!("[OK] Agent created with custom service composition");

    let agent_derived = agent.derive_simple_identity("app", "context").await?;
    println!("• DeviceAgent.derive_simple_identity() (delegates to service): [OK]");

    println!("\nService composition benefits:");
    println!("• Services can be mocked for testing");
    println!("• Different implementations can be swapped");
    println!("• Clear dependency injection pattern");
    println!("• Services are reusable across different agents");

    Ok(())
}

/// Demonstrate clean error handling with service delegation
async fn demonstrate_error_handling() -> Result<()> {
    println!("--- Error Handling with Service Delegation ---");

    // Create registry without all services to demonstrate error handling
    let incomplete_services = Arc::new(ServiceRegistry::new());

    let device_id = DeviceId(Uuid::new_v4());
    let account_id = AccountId(Uuid::new_v4());
    let config = IdentityConfig {
        device_id,
        account_id,
    };

    let agent = DeviceAgent::new(device_id, account_id, config, incomplete_services);

    println!("Created agent with incomplete service registry");

    // Try to use identity service (should fail cleanly)
    match agent.derive_simple_identity("app", "context").await {
        Ok(_) => println!("Unexpected success"),
        Err(AgentError::ProtocolError(msg)) => {
            println!("[OK] Clean error handling: {}", msg);
            println!("  Error propagated from service layer to agent layer");
        }
        Err(e) => println!("Other error: {:?}", e),
    }

    println!("\nError handling benefits:");
    println!("• Errors propagate cleanly through service boundaries");
    println!("• Service errors are converted to agent errors");
    println!("• No complex error handling in high-level methods");
    println!("• Clear error context from each service layer");

    Ok(())
}
