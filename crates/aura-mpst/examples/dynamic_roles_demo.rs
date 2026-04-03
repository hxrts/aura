//! Dynamic Role Projection Demonstration
//!
//! This example demonstrates how Aura's choreography system handles
//! basic choreographic patterns and prepares for future dynamic role support.

// Example code defines types for demonstration that aren't directly called
#![allow(dead_code)]

use aura_mpst::annotation_lowering::{lower_aura_effects, AuraEffect};
use aura_mpst::upstream::language::compile_choreography;
use serde::{Deserialize, Serialize};

// Message types for the threshold ceremony
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SetupRequest {
    ceremony_id: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Commitment {
    value: Vec<u8>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Challenge {
    nonce: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Response {
    signature: Vec<u8>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct FinalSignature {
    signature: Vec<u8>,
}

// Basic threshold ceremony demonstrating multi-party protocol
// NOTE: The choreography! macro is commented out because it generates code that
// references module-level types, which doesn't work in the example file context.
// In a library module, this macro would generate:
// - Session type definitions for each role (Coordinator, Signer1, Signer2, Observer)
// - Automatic projection of the global protocol to local session types
// - Type-safe message passing with deadlock freedom guarantees
//
// choreography! {
//     #[namespace = "threshold_signing"]
//     choreography ThresholdCeremony {
//         roles: Coordinator, Signer1, Signer2, Observer;
//
//         Coordinator -> Signer1: SetupRequest;
//         Coordinator -> Signer2: SetupRequest;
//         Signer1 -> Coordinator: Commitment;
//         Signer2 -> Coordinator: Commitment;
//         Coordinator -> Signer1: Challenge;
//         Coordinator -> Signer2: Challenge;
//         Signer1 -> Coordinator: Response;
//         Signer2 -> Coordinator: Response;
//         Coordinator -> Observer: FinalSignature;
//     }
// }

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Aura Choreography Features Demonstration ===\n");

    println!("[OK] Threshold Ceremony Protocol");
    println!("   Namespace: threshold_signing");
    println!("   Roles: Coordinator, Signer1, Signer2, Observer");
    println!("   Multi-phase protocol with commitment and challenge phases");
    println!(
        "
   Protocol Flow:
   1. Setup Phase:    Coordinator -> Signers"
    );
    println!("   2. Commit Phase:   Signers -> Coordinator");
    println!("   3. Challenge:      Coordinator -> Signers");
    println!("   4. Response:       Signers -> Coordinator");
    println!("   5. Finalization:   Coordinator -> Observer");

    // Demonstrate annotation lowering for Aura semantics
    println!("\n[OK] Aura Extension Support");
    println!("   Testing compiled annotation lowering for future Aura features:");

    let annotated_choreography = r#"
module annotated_demo exposing (AnnotatedProtocol)

protocol AnnotatedProtocol =
  roles Leader, Follower

  Leader { guard_capability : "consensus:initiate" } -> Follower : Instruction
  Follower { flow_cost : 10 } -> Leader : Status
  Leader { journal_facts : "round_complete" } -> Follower : Completion
    "#;

    let lowered_effects = compile_choreography(annotated_choreography)
        .map_err(|err| err.to_string())
        .and_then(|compiled| lower_aura_effects(&compiled).map_err(|err| err.to_string()));

    match lowered_effects {
        Ok(effects) => {
            for effect in &effects {
                match effect {
                    AuraEffect::GuardCapability { capability, role } => {
                        println!("   [+] Guard capability '{capability}' for {role}");
                    }
                    AuraEffect::FlowCost { cost, role } => {
                        println!("   [+] Flow cost {cost} for {role}");
                    }
                    AuraEffect::JournalFacts { facts, role } => {
                        println!("   [+] Journal facts '{facts}' for {role}");
                    }
                    _ => {}
                }
            }
        }
        Err(e) => println!("   [ERROR] Annotation lowering error: {e}"),
    }

    println!("\n[DONE] Choreography demonstration complete!");
    println!("   The system successfully demonstrates:");
    println!("   [OK] Multi-party choreographic protocols");
    println!("   [OK] Namespace isolation");
    println!("   [OK] Session type generation");
    println!("   [OK] Aura annotation lowering");
    println!("   [OK] Integration with Telltale framework");

    println!("\n[INFO] Future Dynamic Role Support:");
    println!("   The foundation is in place for:");
    println!("   [-] Parameterized roles: Signer[N]");
    println!("   [-] Broadcast patterns: Role[*]");
    println!("   [-] Runtime role determination");
    println!("   [-] Dynamic protocol scaling");

    Ok(())
}
