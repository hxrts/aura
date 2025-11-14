//! Basic usage example of choreography! macro
//!
//! This example demonstrates the basic syntax for choreographic programming
//! with Aura-specific extensions.

use serde::{Serialize, Deserialize};
use rumpsteak_aura_choreography::compiler::parse_dsl;

// Message type definitions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestMessage {
    pub data: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestResponse {
    pub status: String,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Basic Choreography Example ===\n");

    // Define a simple choreography using DSL string
    let choreography_dsl = r#"
    choreography BasicTest {
        roles: Alice, Bob;
        Alice -> Bob: TestMessage;
        Bob -> Alice: TestResponse;
    }
    "#;

    println!("Parsing choreography DSL...");
    
    // Parse the choreography
    let choreography = parse_dsl(choreography_dsl)?;
    
    println!("✓ Choreography parsed successfully!");
    println!("Choreography name: {}", choreography.name);
    println!("Roles: {:?}", choreography.roles.iter().map(|r| &r.name).collect::<Vec<_>>());
    
    // Validate the choreography
    choreography.validate()?;
    println!("✓ Choreography validation passed!");
    
    println!("\nThis example demonstrates:");
    println!("- DSL-based choreography specification");
    println!("- Role declaration (Alice, Bob)");
    println!("- Message passing specification");
    println!("- Compile-time validation");
    
    println!("\n=== Example Complete ===");
    Ok(())
}
