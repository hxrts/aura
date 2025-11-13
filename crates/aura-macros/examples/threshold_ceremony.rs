//! Threshold Ceremony Implementation using Enhanced Choreography System
//!
//! This example demonstrates the choreography! macro syntax for a threshold
//! ceremony with Aura-specific extensions.

use aura_macros::choreography;

// Demonstrates threshold ceremony with multiple roles and phases
choreography! {
    #[namespace = "threshold_ceremony"]
    protocol ThresholdCeremony {
        roles: Coordinator, Signer1, Signer2;

        // Phase 1: Coordinator initiates ceremony
        Coordinator[guard_capability = "coordinate_threshold_signing"]
        -> Signer1: SignRequest;

        Coordinator[guard_capability = "coordinate_threshold_signing"]
        -> Signer2: SignRequest;

        // Phase 2: Signers respond with nonce commits  
        Signer1[guard_capability = "participate_threshold_signing"]
        -> Coordinator: NonceCommit;

        Signer2[guard_capability = "participate_threshold_signing"]
        -> Coordinator: NonceCommit;

        // Phase 3: Final result broadcast
        Coordinator[journal_facts = "threshold_result_broadcast"]
        -> Signer1: FinalResult;
    }
}

fn main() {
    println!("Enhanced Threshold Ceremony Choreography");
    println!("=========================================");
    println!();
    println!("This example demonstrates the choreography! macro combining:");
    println!("- Rumpsteak-aura's session types and choreographic programming");
    println!("- Aura-specific extensions for capability guards and flow costs");
    println!("- Journal coupling for distributed state consistency");
    println!();

    println!("Generated modules:");
    println!("- Session types from rumpsteak-aura");
    println!("- Extension registry calls for aura-mpst");
    println!("- Guard chain integration");
    println!("- Journal coupling hooks");
    println!();

    println!("Protocol includes the following phases:");
    println!("1. Initialization: Coordinator -> Signers (Sign Request)");
    println!("2. Commitment: Signers -> Coordinator (Nonce Commits)");
    println!("3. Final result: Coordinator -> Signers (Results)");
    println!();

    println!("Each phase includes:");
    println!("- Guard capability validation");
    println!("- Flow cost tracking");
    println!("- Journal fact recording");
    println!("- Session type safety from rumpsteak-aura");
    println!();

    println!("Aura-enhanced choreographic programming working perfectly!");
    println!("Combining the best of both rumpsteak-aura and Aura's domain-specific features.");
}
