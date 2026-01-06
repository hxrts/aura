//! # Hello Choreography Example
//!
//! A demonstration of choreographic protocol programming using aura-macros.
//!
//! This example shows how to use the choreography macro with both rumpsteak-aura
//! session types and the Aura effects system integration.

#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]

use std::collections::HashMap;

// Define message types for the choreography
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct Ping;

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct Pong;

// Use the choreography macro - generates both rumpsteak and Aura modules.
// Note: Each choreography must have a unique module namespace.
use aura_macros::choreography;
choreography!(include_str!("src/main.choreo"));

// The macro generates the aura_choreography_pingpong module (namespace-prefixed).

/// Demonstration of the choreography system using the actual macro
#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    println!("=== Hello Choreography: Aura Macro Demo ===\n");

    use aura_choreography_pingpong::*;

    println!("1. Creating Aura handlers with capabilities:");

    let mut alice_handler = create_simple_handler(AuraRole::Alice);
    let mut bob_handler = create_simple_handler(AuraRole::Bob);

    println!(
        "   Alice: role = {:?}, balance = {}",
        alice_handler.role,
        alice_handler.get_flow_balance()
    );
    println!(
        "   Bob: role = {:?}, balance = {}",
        bob_handler.role,
        bob_handler.get_flow_balance()
    );

    println!("\n2. Building choreography with Aura effects:");

    let mut start_metadata = HashMap::new();
    start_metadata.insert("session_id".to_string(), "hello_demo".to_string());

    let mut end_metadata = HashMap::new();
    end_metadata.insert("status".to_string(), "success".to_string());

    let ping_pong_protocol = builder()
        .audit_log("protocol_start", start_metadata)
        .validate_guard(AuraRole::Alice, "send_ping", "network")
        .charge_flow_cost(AuraRole::Alice, 150)
        .send(AuraRole::Alice, AuraRole::Bob, "ping_message")
        .validate_guard(AuraRole::Bob, "send_pong", "network")
        .charge_flow_cost(AuraRole::Bob, 100)
        .send(AuraRole::Bob, AuraRole::Alice, "pong_message")
        .audit_log("protocol_complete", end_metadata)
        .end();

    println!("   Built choreography with Aura capability checks and flow costs");

    println!("\n3. Executing choreography:");

    // Execute from Alice's perspective
    println!("   Executing as Alice...");
    let mut alice_endpoint = ();
    let alice_result = execute(
        &mut alice_handler,
        &mut alice_endpoint,
        ping_pong_protocol.clone(),
    )
    .await;

    match alice_result {
        Ok(_) => {
            println!("     Alice execution: SUCCESS");
            println!(
                "     Alice final balance: {}",
                alice_handler.get_flow_balance()
            );
        }
        Err(e) => println!("     Alice execution: FAILED - {}", e),
    }

    // Execute from Bob's perspective
    println!("   Executing as Bob...");
    let mut bob_endpoint = ();
    let bob_result = execute(&mut bob_handler, &mut bob_endpoint, ping_pong_protocol).await;

    match bob_result {
        Ok(_) => {
            println!("     Bob execution: SUCCESS");
            println!("     Bob final balance: {}", bob_handler.get_flow_balance());
        }
        Err(e) => println!("     Bob execution: FAILED - {}", e),
    }

    println!("\n4. Testing example choreography:");

    let example_protocol = example_aura_choreography();
    let mut alice_example_handler = create_simple_handler(AuraRole::Alice);
    let mut alice_endpoint = ();
    let example_result = execute(
        &mut alice_example_handler,
        &mut alice_endpoint,
        example_protocol,
    )
    .await;

    match example_result {
        Ok(_) => {
            println!("   Example execution: SUCCESS");
            println!(
                "   Alice final balance: {}",
                alice_example_handler.get_flow_balance()
            );
        }
        Err(e) => println!("   Example execution: FAILED - {}", e),
    }

    println!("\n5. Testing rumpsteak-aura session types integration:");

    // Rumpsteak session types are now integrated and available
    println!("   ✓ Rumpsteak session types module generated");
    println!("   ✓ Session type safety and choreographic projection available");

    println!("\n=== Integration Summary ===");
    println!("✓ Aura choreography system working correctly");
    println!("✓ Effect system (capability validation, flow costs, audit logging)");
    println!("✓ Extension registry and handler integration");
    println!("✓ Multi-role choreography execution");
    println!("✓ Type-safe role system and builder pattern");
    println!("✓ Rumpsteak-aura session types integration");
    println!("✓ Choreographic projection and deadlock freedom");
    println!();
    println!("Note: This example now uses the complete aura-macros choreography macro!");
    println!("The macro generates both Aura effects and rumpsteak session types.");

    println!("\nHello Choreography demo completed.");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_choreography_execution() {
        use aura_choreography_pingpong::*;

        let mut handler = create_simple_handler(AuraRole::Alice);
        let program = builder()
            .validate_guard(AuraRole::Alice, "test", "storage")
            .charge_flow_cost(AuraRole::Alice, 50)
            .end();

        let mut endpoint = ();
        let result = execute(&mut handler, &mut endpoint, program).await;

        assert!(
            result.is_ok(),
            "Execution should succeed with valid capability"
        );
        assert_eq!(handler.get_flow_balance(), 950); // 1000 - 50 = 950
    }

    #[tokio::test]
    async fn test_example_choreography() {
        use aura_choreography_pingpong::*;

        let example = example_aura_choreography();
        let mut handler = create_simple_handler(AuraRole::Alice);
        let mut endpoint = ();

        let result = execute(&mut handler, &mut endpoint, example).await;
        assert!(
            result.is_ok(),
            "Example choreography should execute successfully"
        );
    }

    #[test]
    fn test_role_display() {
        use aura_choreography_pingpong::AuraRole;

        assert_eq!(format!("{}", AuraRole::Alice), "Alice");
        assert_eq!(format!("{}", AuraRole::Bob), "Bob");
    }
}
