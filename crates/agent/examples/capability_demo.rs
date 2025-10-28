//! Capability-Driven Architecture Demo
//!
//! This example demonstrates the different agent types and their
//! capability-driven architecture patterns.

use aura_agent::{
    core::{CapabilityAgent, IntegratedAgent},
    services::{AccountService, IdentityService},
    Result,
};
use aura_types::{AccountId, DeviceId};
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::init();

    println!("=== Aura Agent: Capability-Driven Architecture Demo ===\n");

    demo_capability_agent().await?;
    println!();
    demo_integrated_agent().await?;
    println!();
    demo_service_layer().await?;

    Ok(())
}

/// Demonstrate pure capability-driven agent
async fn demo_capability_agent() -> Result<()> {
    println!("--- CapabilityAgent Demo ---");
    println!("Pure capability-driven agent with no external dependencies");

    let device_id = DeviceId(Uuid::new_v4());
    let account_id = AccountId(Uuid::new_v4());

    // Create pure capability agent
    let mut agent = CapabilityAgent::new(device_id, account_id);

    println!("[OK] Created CapabilityAgent");

    // Demonstrate capability-based operations
    // Note: These would be implemented in the actual CapabilityAgent
    println!("• Supports: check_capability(), create_group(), encrypt(), decrypt()");
    println!("• Zero external dependencies");
    println!("• Ideal for: testing, embedded systems, library integration");

    Ok(())
}

/// Demonstrate integrated agent with full system capabilities
async fn demo_integrated_agent() -> Result<()> {
    println!("--- IntegratedAgent Demo ---");
    println!("Full system integration with transport and storage");

    let device_id = DeviceId(Uuid::new_v4());
    let account_id = AccountId(Uuid::new_v4());
    let storage_path = "/tmp/aura_demo";

    // Create integrated agent
    let agent = IntegratedAgent::new(device_id, account_id, storage_path).await?;

    println!(
        "[OK] Created IntegratedAgent with storage at: {}",
        storage_path
    );

    // Demonstrate integrated operations
    println!("• Supports: All CapabilityAgent features plus:");
    println!("  - bootstrap() - Account initialization");
    println!("  - network_connect() - P2P networking");
    println!("  - store() / retrieve() - Encrypted storage");
    println!("  - Network-aware capability delegation");

    Ok(())
}

/// Demonstrate direct service layer usage
async fn demo_service_layer() -> Result<()> {
    println!("--- Service Layer Demo ---");
    println!("Direct usage of service layer for custom integrations");

    // Create services directly
    let identity_service = IdentityService::new();
    let account_service = AccountService::new();

    println!("[OK] Created IdentityService and AccountService");

    // Demonstrate service capabilities
    println!("• IdentityService: derive_key(), issue_tickets(), manage_contexts()");
    println!("• AccountService: bootstrap(), add_device(), session_management()");
    println!("• Services can be used independently or composed");
    println!("• Perfect for custom integrations and testing");

    Ok(())
}
