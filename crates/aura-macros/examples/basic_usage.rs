//! Basic usage example of choreography! macro
//!
//! This example demonstrates the basic syntax and annotation support
//! of the choreography! macro without runtime dependencies.

use aura_macros::choreography;

// Example choreography with Aura annotations
choreography! {
    #[namespace = "basic_test"]
    protocol BasicTest {
        roles: Alice, Bob;

        Alice[guard_capability = "send_message",
              flow_cost = 150,
              journal_facts = "message_sent"]
        -> Bob: TestMessage(String);

        Bob[guard_capability = "respond",
            flow_cost = 100]
        -> Alice: TestResponse(u32);
    }
}

fn main() {
    println!("Basic choreography macro example compiled successfully!");

    // This example focuses on compile-time validation only
    // Runtime integration is demonstrated in aura-mpst crate tests

    println!("Generated choreography code includes:");
    println!("- Session type definitions");
    println!("- Role-specific projections");
    println!("- Extension registry calls for guards and journal facts");
    println!("- Integration hooks for aura-mpst runtime");
}
