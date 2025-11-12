//! Threshold Ceremony Implementation using Enhanced Choreography System
//!
//! This example demonstrates how to use the aura_choreography! macro to implement
//! a threshold signature ceremony with proper Aura annotations for guards,
//! flow costs, and journal coupling.

use aura_macros::aura_choreography;

// Enhanced threshold ceremony choreography with Aura annotations
aura_choreography! {
    #[namespace = "threshold_ceremony"]
    protocol ThresholdCeremony {
        roles: Coordinator, Signer1, Signer2, Signer3, Observer1, Observer2;

        // Phase 1: Coordinator initiates ceremony with sign request
        Coordinator[guard_capability = "coordinate_threshold_signing",
                   flow_cost = 200,
                   journal_facts = "threshold_ceremony_initiated"]
        -> Signer1: SignRequest(String);

        Coordinator -> Signer2: SignRequest(String);
        Coordinator -> Signer3: SignRequest(String);

        // Phase 2: Signers commit nonces
        Signer1[guard_capability = "participate_threshold_signing",
               flow_cost = 150,
               journal_facts = "nonce_committed"]
        -> Coordinator: NonceCommit(String);

        Signer2[guard_capability = "participate_threshold_signing",
               flow_cost = 150]
        -> Coordinator: NonceCommit(String);

        Signer3[guard_capability = "participate_threshold_signing",
               flow_cost = 150]
        -> Coordinator: NonceCommit(String);

        // Phase 3: Coordinator broadcasts challenge
        Coordinator[guard_capability = "coordinate_threshold_signing",
                   flow_cost = 100,
                   journal_facts = "challenge_broadcast"]
        -> Signer1: ChallengeRequest(String);

        Coordinator -> Signer2: ChallengeRequest(String);
        Coordinator -> Signer3: ChallengeRequest(String);

        // Phase 4: Signers submit partial signatures
        Signer1[guard_capability = "submit_threshold_signature",
               flow_cost = 200,
               journal_facts = "partial_signature_submitted"]
        -> Coordinator: PartialSig(String);

        Signer2[guard_capability = "submit_threshold_signature",
               flow_cost = 200]
        -> Coordinator: PartialSig(String);

        Signer3[guard_capability = "submit_threshold_signature",
               flow_cost = 200]
        -> Coordinator: PartialSig(String);

        // Phase 5: Coordinator broadcasts result to all participants
        Coordinator[guard_capability = "broadcast_threshold_result",
                   flow_cost = 50,
                   journal_facts = "threshold_result_broadcast",
                   journal_merge = true]
        -> Signer1: AttestedResult(String);

        Coordinator -> Signer2: AttestedResult(String);
        Coordinator -> Signer3: AttestedResult(String);
        Coordinator -> Observer1: AttestedResult(String);
        Coordinator -> Observer2: AttestedResult(String);
    }
}

fn main() {
    println!("=== Enhanced Threshold Ceremony Choreography ===");
    println!();
    println!("This example demonstrates a threshold signature ceremony");
    println!("implemented using the aura_choreography! macro with:");
    println!("- Guard capability annotations for access control");
    println!("- Flow cost tracking for resource management");
    println!("- Journal coupling for distributed state consistency");
    println!();

    // Test that the choreography module was generated
    let _protocol = threshold_ceremony::ThresholdCeremony::new();
    println!("Threshold ceremony protocol generated successfully");

    // Test basic execution framework
    match threshold_ceremony::execute_protocol() {
        Ok(_) => println!("Protocol execution framework ready"),
        Err(e) => println!("Protocol execution failed: {:?}", e),
    }

    println!();
    println!("Protocol includes the following phases:");
    println!("1. Initialization: Coordinator → Signers (Sign Request)");
    println!("2. Commitment: Signers → Coordinator (Nonce Commits)");
    println!("3. Challenge: Coordinator → Signers (Challenge Broadcast)");
    println!("4. Signing: Signers → Coordinator (Partial Signatures)");
    println!("5. Broadcast: Coordinator → All (Final Result)");
    println!();
    println!("Each phase includes appropriate guard capabilities and flow costs!");
}
