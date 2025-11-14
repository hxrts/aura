//! Threshold Ceremony Implementation using DSL
//!
//! This example demonstrates parsing a threshold ceremony protocol
//! using the rumpsteak-aura DSL parser.

use serde::{Serialize, Deserialize};
use rumpsteak_aura_choreography::compiler::parse_dsl;

// Message type definitions for threshold ceremony
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignRequest {
    pub ceremony_id: String,
    pub message_hash: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NonceCommit {
    pub commitment: Vec<u8>,
    pub signer_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinalResult {
    pub signature: Vec<u8>,
    pub success: bool,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Threshold Ceremony Choreography ===\n");

    // Define the threshold ceremony protocol using DSL
    let ceremony_dsl = r#"
    choreography ThresholdCeremony {
        roles: Coordinator, Signer1, Signer2;

        // Phase 1: Coordinator initiates ceremony
        Coordinator -> Signer1: SignRequest;
        Coordinator -> Signer2: SignRequest;

        // Phase 2: Signers respond with nonce commits  
        Signer1 -> Coordinator: NonceCommit;
        Signer2 -> Coordinator: NonceCommit;

        // Phase 3: Final result broadcast
        Coordinator -> Signer1: FinalResult;
        Coordinator -> Signer2: FinalResult;
    }
    "#;

    println!("Parsing threshold ceremony DSL...");
    
    // Parse the choreography
    let choreography = parse_dsl(ceremony_dsl)?;
    
    println!("✓ Choreography parsed successfully!");
    println!("Protocol: {}", choreography.name);
    println!("Roles: {:?}", choreography.roles.iter().map(|r| &r.name).collect::<Vec<_>>());
    if let Some(namespace) = &choreography.namespace {
        println!("Namespace: {}", namespace);
    }
    
    // Validate the choreography
    choreography.validate()?;
    println!("✓ Protocol validation passed!");
    
    println!("\n=== Protocol Structure ===");
    println!("This threshold ceremony includes:");
    println!("1. Initialization: Coordinator → Signers (Sign Request)");
    println!("2. Commitment: Signers → Coordinator (Nonce Commits)"); 
    println!("3. Finalization: Coordinator → Signers (Final Results)");
    
    println!("\n=== Note on Aura Extensions ===");
    println!("To add Aura-specific features like capability guards, flow costs,");
    println!("and journal facts, use the aura_macros::choreography! macro instead:");
    println!();
    println!("choreography! {{");
    println!("    choreography ThresholdCeremony {{");
    println!("        roles: Coordinator, Signer1, Signer2;");
    println!("        Coordinator[@guard_capability = \"coordinate_signing\", @flow_cost = 200]");
    println!("        -> Signer1: SignRequest;");
    println!("        // ... etc");
    println!("    }}");
    println!("}}");
    
    println!("\n=== Example Complete ===");
    Ok(())
}
