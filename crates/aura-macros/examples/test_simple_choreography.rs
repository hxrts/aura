//! Simple test of the choreography macro without annotations
//! This should test whether the import injection is working correctly

use aura_macros::choreography;
use serde::{Serialize, Deserialize};

// Import all the rumpsteak-aura types and macros needed by the generated code
use rumpsteak_aura::{End, Receive, Send};
use rumpsteak_aura_macros::{Role, Roles, session};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimpleMessage {
    pub data: String,
}

// Test 1: Very basic choreography with no annotations
choreography! {
    choreography BasicTest {
        roles: Alice, Bob;
        
        Alice -> Bob: SimpleMessage;
    }
}

fn main() {
    println!("Simple choreography test compiled successfully!");
}