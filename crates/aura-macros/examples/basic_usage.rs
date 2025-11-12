//! Basic usage example of aura_choreography! macro

use aura_macros::aura_choreography;

// Basic serde dependency for generated code
extern crate serde;

// Example choreography with Aura annotations
aura_choreography! {
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
    
    // Test that the basic protocol module was generated
    let _protocol = basic_test::BasicTest::new();
    
    // Test the protocol execution (placeholder)
    let _result = basic_test::execute_protocol().unwrap();
    
    println!("Generated choreography module is accessible!");
}