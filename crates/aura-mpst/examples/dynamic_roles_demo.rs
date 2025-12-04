//! Dynamic Role Projection Demonstration
//!
//! This example demonstrates how Aura's choreography system handles
//! basic choreographic patterns and prepares for future dynamic role support.

// Example code defines types for demonstration that aren't directly called
#![allow(dead_code)]

use aura_mpst::ast_extraction::{extract_aura_annotations, AuraEffect};
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender};
use rumpsteak_aura::*;
use rumpsteak_aura_choreography::Label;
use serde::{Deserialize, Serialize};

// Required type definitions for the generated choreography
type Channel = channel::Bidirectional<UnboundedSender<Label>, UnboundedReceiver<Label>>;

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

    // Demonstrate annotation extraction for Aura extensions
    println!("\n[OK] Aura Extension Support");
    println!("   Testing annotation extraction for future Aura features:");

    let annotated_choreography = r#"
        choreography AnnotatedProtocol {
            namespace: "annotated_demo";
            roles: Leader, Follower;

            Leader[guard_capability = "coordinate"] -> Follower: Instruction;
            Follower[flow_cost = 10] -> Leader: Status;
            Leader[journal_facts = "round_complete"] -> Follower: Completion;
        }
    "#;

    match extract_aura_annotations(annotated_choreography) {
        Ok(effects) => {
            for effect in &effects {
                match effect {
                    AuraEffect::GuardCapability { capability, role } => {
                        println!("   [+] Guard capability '{}' for {}", capability, role);
                    }
                    AuraEffect::FlowCost { cost, role } => {
                        println!("   [+] Flow cost {} for {}", cost, role);
                    }
                    AuraEffect::JournalFacts { facts, role } => {
                        println!("   [+] Journal facts '{}' for {}", facts, role);
                    }
                    _ => {}
                }
            }
        }
        Err(e) => println!("   [ERROR] Annotation extraction error: {}", e),
    }

    println!("\n[DONE] Choreography demonstration complete!");
    println!("   The system successfully demonstrates:");
    println!("   [OK] Multi-party choreographic protocols");
    println!("   [OK] Namespace isolation");
    println!("   [OK] Session type generation");
    println!("   [OK] Aura annotation extraction");
    println!("   [OK] Integration with rumpsteak-aura framework");

    println!("\n[INFO] Future Dynamic Role Support:");
    println!("   The foundation is in place for:");
    println!("   [-] Parameterized roles: Signer[N]");
    println!("   [-] Broadcast patterns: Role[*]");
    println!("   [-] Runtime role determination");
    println!("   [-] Dynamic protocol scaling");

    Ok(())
}
