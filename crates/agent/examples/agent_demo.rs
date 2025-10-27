//! Agent Demonstration
//!
//! This example shows how to use the agent implementation
//! with compile-time state safety and generic transport/storage.

use aura_agent::{
    Agent, AgentFactory, BootstrapConfig, CoordinatingAgent, ProtocolCompleted, ProtocolStatus,
    Storage, Transport, UnifiedAgent,
};
use aura_types::{DeviceId, AccountId, GuardianId};
use std::sync::Arc;
use uuid::Uuid;

/// Example transport implementation
#[derive(Debug)]
struct ExampleTransport {
    device_id: DeviceId,
}

impl Transport for ExampleTransport {
    fn device_id(&self) -> DeviceId {
        self.device_id
    }
}

/// Example storage implementation
#[derive(Debug)]
struct ExampleStorage {
    account_id: AccountId,
}

impl Storage for ExampleStorage {
    fn account_id(&self) -> AccountId {
        self.account_id
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::init();

    println!("Agent Demonstration");
    println!("======================");

    // Setup
    let device_id = DeviceId::new(Uuid::new_v4());
    let account_id = AccountId::new(Uuid::new_v4());

    println!("Device ID: {}", device_id);
    println!("Account ID: {}", account_id);

    // Step 1: Create uninitialized agent
    println!("\nStep 1: Creating uninitialized agent...");

    let transport = Arc::new(ExampleTransport { device_id });
    let storage = Arc::new(ExampleStorage { account_id });

    let uninit_agent =
        AgentFactory::create_production(device_id, account_id, transport, storage).await?;

    println!("[OK] Uninitialized agent created");
    println!("   Device ID: {}", uninit_agent.device_id());
    println!("   Account ID: {}", uninit_agent.account_id());

    // Step 2: Demonstrate compile-time safety
    println!("\nStep 2: Demonstrating compile-time safety...");

    // This would NOT compile:
    // uninit_agent.store_data(b"data", vec!["read".to_string()]).await?;
    println!("[ERROR] Cannot call store_data() on uninitialized agent (compile-time error)");

    // Step 3: Bootstrap the agent
    println!("\nStep 3: Bootstrapping agent...");

    let bootstrap_config = BootstrapConfig {
        threshold: 2,
        share_count: 3,
        parameters: [
            ("network".to_string(), "testnet".to_string()),
            ("encryption".to_string(), "aes256".to_string()),
        ]
        .into_iter()
        .collect(),
    };

    let idle_agent = uninit_agent.bootstrap(bootstrap_config).await?;
    println!("[OK] Agent bootstrapped successfully");

    // Step 4: Perform operations in idle state
    println!("\nStep 4: Performing operations in idle state...");

    // Identity derivation
    println!("Deriving identity...");
    match idle_agent.derive_identity("demo-app", "user-session").await {
        Ok(_identity) => println!("[OK] Identity derived successfully"),
        Err(e) => println!("[ERROR] Identity derivation failed: {}", e),
    }

    // Data storage
    println!("Storing data...");
    let test_data = b"This is sensitive user data";
    let capabilities = vec!["read".to_string(), "write".to_string()];

    match idle_agent.store_data(test_data, capabilities).await {
        Ok(_data_id) => println!("[OK] Data stored successfully"),
        Err(e) => println!("[ERROR] Data storage failed: {}", e),
    }

    // Step 5: Initiate a long-running protocol
    println!("\nStep 5: Initiating recovery protocol...");

    let recovery_params = serde_json::json!({
        "recovery_type": "threshold",
        "required_shares": 2,
        "timeout_minutes": 30
    });

    let coordinating_agent = idle_agent.initiate_recovery(recovery_params).await?;
    println!("[OK] Recovery protocol initiated");

    // Step 6: Demonstrate restricted API in coordinating state
    println!("\nStep 6: Demonstrating restricted API in coordinating state...");

    // This would NOT compile:
    // coordinating_agent.initiate_resharing(3, vec![device_id]).await?;
    println!("[ERROR] Cannot initiate resharing while coordinating (compile-time error)");

    // But we can check protocol status
    println!("Checking protocol status...");
    match coordinating_agent.check_protocol_status().await? {
        ProtocolStatus::InProgress => println!("[INFO] Protocol is still running"),
        ProtocolStatus::Completed => println!("[OK] Protocol completed"),
        ProtocolStatus::Failed(err) => println!("[ERROR] Protocol failed: {}", err),
    }

    // Step 7: Complete the protocol
    println!("\nStep 7: Completing protocol...");

    // Simulate protocol completion
    let witness = ProtocolCompleted {
        protocol_id: Uuid::new_v4(),
        result: serde_json::json!({
            "status": "success",
            "shares_recovered": 2,
            "new_threshold": 2
        }),
    };

    let idle_agent_again = coordinating_agent.finish_coordination(witness);
    println!("[OK] Protocol completed, agent back to idle state");

    // Step 8: Demonstrate agent traits
    println!("\nStep 8: Demonstrating agent traits...");

    // Use agent through trait
    demonstrate_agent_trait(&idle_agent_again).await?;

    println!("\nDemonstration completed successfully!");
    println!("   The agent provides:");
    println!("   [OK] Compile-time state safety");
    println!("   [OK] Generic transport/storage");
    println!("   [OK] Unified API across all capabilities");
    println!("   [OK] Zero-cost abstractions");

    Ok(())
}

/// Demonstrate using the agent through its trait interface
async fn demonstrate_agent_trait<A: Agent>(agent: &A) -> Result<(), Box<dyn std::error::Error>> {
    println!("Using agent through trait interface:");

    println!("   Device ID: {}", agent.device_id());
    println!("   Account ID: {}", agent.account_id());

    // Try to derive identity through trait
    match agent.derive_identity("trait-demo", "context").await {
        Ok(_) => println!("[OK] Identity derivation via trait succeeded"),
        Err(e) => println!("[ERROR] Identity derivation via trait failed: {}", e),
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_agent_state_transitions() {
        let device_id = DeviceId::new(Uuid::new_v4());
        let account_id = AccountId::new(Uuid::new_v4());

        // Create uninitialized agent
        let uninit_agent = AgentFactory::create_test(device_id, account_id)
            .await
            .unwrap();

        // Bootstrap to idle
        let config = BootstrapConfig {
            threshold: 2,
            share_count: 3,
            parameters: Default::default(),
        };
        let idle_agent = uninit_agent.bootstrap(config).await.unwrap();

        // Verify we can access basic info
        assert_eq!(idle_agent.device_id(), device_id);
        assert_eq!(idle_agent.account_id(), account_id);

        // Start coordination
        let coordinating_agent = idle_agent
            .initiate_recovery(serde_json::json!({}))
            .await
            .unwrap();

        // Check status
        let status = coordinating_agent.check_protocol_status().await.unwrap();
        assert!(matches!(status, ProtocolStatus::InProgress));

        // Complete coordination
        let witness = ProtocolCompleted {
            protocol_id: Uuid::new_v4(),
            result: serde_json::json!({"success": true}),
        };
        let _idle_again = coordinating_agent.finish_coordination(witness);

        // Test passes if we get here - demonstrates type safety
    }

    #[tokio::test]
    async fn test_agent_traits() {
        let device_id = DeviceId::new(Uuid::new_v4());
        let account_id = AccountId::new(Uuid::new_v4());

        let uninit_agent = AgentFactory::create_test(device_id, account_id)
            .await
            .unwrap();
        let config = BootstrapConfig {
            threshold: 1,
            share_count: 1,
            parameters: Default::default(),
        };
        let idle_agent = uninit_agent.bootstrap(config).await.unwrap();

        // Test agent trait
        test_agent_interface(&idle_agent).await;
    }

    async fn test_agent_interface<A: Agent>(agent: &A) {
        // This function accepts any type implementing Agent
        let _device_id = agent.device_id();
        let _account_id = agent.account_id();

        // Try operations (they may fail due to mock implementation)
        let _ = agent.derive_identity("test", "context").await;
        let _ = agent.store_data(b"test", vec!["read".to_string()]).await;
    }
}
