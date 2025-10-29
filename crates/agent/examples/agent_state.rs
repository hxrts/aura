//! Agent State Example
//!
//! This example demonstrates the core agent concepts and types
//! that are currently available in the Aura agent crate.

use aura_agent::{DerivedIdentity, DeviceAttestation, Result, SecurityLevel};
use aura_types::{AccountId, DeviceId};

/// Mock agent state for demonstration
#[derive(Debug)]
struct DemoAgent {
    device_id: DeviceId,
    account_id: AccountId,
    state: AgentState,
}

#[derive(Debug)]
enum AgentState {
    Uninitialized,
    Idle,
}

impl DemoAgent {
    fn new(device_id: DeviceId, account_id: AccountId) -> Self {
        Self {
            device_id,
            account_id,
            state: AgentState::Uninitialized,
        }
    }

    fn bootstrap(&mut self) -> Result<()> {
        match self.state {
            AgentState::Uninitialized => {
                self.state = AgentState::Idle;
                Ok(())
            }
            _ => Err(aura_agent::AgentError::agent_invalid_state(
                "Agent already bootstrapped",
            )),
        }
    }

    fn derive_identity(&self, app_id: &str, context: &str) -> Result<DerivedIdentity> {
        match self.state {
            AgentState::Idle => Ok(DerivedIdentity {
                app_id: app_id.to_string(),
                context: context.to_string(),
                identity_key: vec![0u8; 32], // Mock 32-byte key
                proof: vec![0u8; 64],        // Mock 64-byte proof
            }),
            _ => Err(aura_agent::AgentError::agent_invalid_state(
                "Cannot derive identity in current state",
            )),
        }
    }

    fn get_device_attestation(&self) -> DeviceAttestation {
        DeviceAttestation {
            platform: "Demo Platform".to_string(),
            device_id: self.device_id.to_string(),
            security_features: vec![
                "Mock Hardware Security".to_string(),
                "Demo Encryption".to_string(),
            ],
            security_level: SecurityLevel::Software,
            attestation_data: [
                ("demo".to_string(), "true".to_string()),
                ("version".to_string(), "1.0".to_string()),
                ("account_id".to_string(), self.account_id.to_string()),
            ]
            .into_iter()
            .collect(),
        }
    }
}

#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    println!("Agent Demonstration");
    println!("======================");

    // Setup
    let device_id = DeviceId::new();
    let account_id = AccountId::new();

    println!("Device ID: {}", device_id);
    println!("Account ID: {}", account_id);

    // Step 1: Create uninitialized agent
    println!("\nStep 1: Creating demo agent...");
    let mut agent = DemoAgent::new(device_id, account_id);
    println!("[OK] Demo agent created in {:?} state", agent.state);

    // Step 2: Demonstrate state safety
    println!("\nStep 2: Demonstrating state safety...");

    // This should fail - cannot derive identity when uninitialized
    match agent.derive_identity("demo-app", "user-session") {
        Ok(_) => println!("[ERROR] Should not be able to derive identity when uninitialized"),
        Err(e) => println!("[OK] Correctly prevented operation in invalid state: {}", e),
    }

    // Step 3: Bootstrap the agent
    println!("\nStep 3: Bootstrapping agent...");
    agent.bootstrap()?;
    println!("[OK] Agent bootstrapped to {:?} state", agent.state);

    // Step 4: Perform operations in idle state
    println!("\nStep 4: Performing operations in idle state...");

    // Identity derivation
    println!("Deriving identity...");
    match agent.derive_identity("demo-app", "user-session") {
        Ok(identity) => {
            println!("[OK] Identity derived successfully:");
            println!("     App ID: {}", identity.app_id);
            println!("     Context: {}", identity.context);
            println!("     Key size: {} bytes", identity.identity_key.len());
        }
        Err(e) => println!("[ERROR] Identity derivation failed: {}", e),
    }

    // Step 5: Get device attestation
    println!("\nStep 5: Getting device attestation...");
    let attestation = agent.get_device_attestation();
    println!("[OK] Device attestation retrieved:");
    println!("     Platform: {}", attestation.platform);
    println!("     Security Level: {:?}", attestation.security_level);
    println!("     Features: {:?}", attestation.security_features);

    // Step 6: Demonstrate serialization
    println!("\nStep 6: Testing serialization...");
    let identity = agent.derive_identity("serialization-test", "context")?;
    let serialized = serde_json::to_string_pretty(&identity)?;
    println!("[OK] Identity serialized:");
    println!("{}", serialized);

    println!("\nDemonstration completed successfully!");
    println!("\nThis demo shows:");
    println!("   [OK] Agent state management");
    println!("   [OK] State-based operation safety");
    println!("   [OK] Identity derivation");
    println!("   [OK] Device attestation");
    println!("   [OK] JSON serialization");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_demo_agent_state_transitions() {
        let device_id = DeviceId::new();
        let account_id = AccountId::new();

        // Create uninitialized agent
        let mut agent = DemoAgent::new(device_id, account_id);

        // Should start uninitialized
        matches!(agent.state, AgentState::Uninitialized);

        // Cannot derive identity when uninitialized
        assert!(agent.derive_identity("test", "context").is_err());

        // Bootstrap to idle
        agent.bootstrap().unwrap();
        matches!(agent.state, AgentState::Idle);

        // Now can derive identity
        let identity = agent.derive_identity("test", "context").unwrap();
        assert_eq!(identity.app_id, "test");
        assert_eq!(identity.context, "context");

        // Can get attestation
        let attestation = agent.get_device_attestation();
        assert_eq!(attestation.platform, "Demo Platform");
    }

    #[test]
    fn test_derived_identity_serialization() {
        let identity = DerivedIdentity {
            app_id: "test-app".to_string(),
            context: "test-context".to_string(),
            identity_key: vec![1, 2, 3, 4],
            proof: vec![5, 6, 7, 8],
        };

        // Test JSON serialization
        let json = serde_json::to_string(&identity).unwrap();
        let deserialized: DerivedIdentity = serde_json::from_str(&json).unwrap();

        assert_eq!(identity.app_id, deserialized.app_id);
        assert_eq!(identity.context, deserialized.context);
        assert_eq!(identity.identity_key, deserialized.identity_key);
        assert_eq!(identity.proof, deserialized.proof);
    }
}
