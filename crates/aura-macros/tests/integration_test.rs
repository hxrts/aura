//! Integration tests for aura-macros
//!
//! These tests verify that the proc macros compile and generate valid code.

use aura_macros::{choreography, aura_effect_handlers};
use rumpsteak_aura::*;
use futures::channel::mpsc::{UnboundedSender, UnboundedReceiver};
use serde::{Deserialize, Serialize};

// Type definitions required by the generated code
#[allow(dead_code)]
type Channel = channel::Bidirectional<UnboundedSender<Label>, UnboundedReceiver<Label>>;

#[derive(Message)]
#[allow(dead_code)]
enum Label {
    Message(Message),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub data: String,
}

// Test that the macro compiles and generates valid types
// This generates session types and role enums for a basic protocol
choreography! {
    #[namespace = "test"]
    choreography TestProtocol {
        roles: Alice, Bob;
        Alice -> Bob: Message;
    }
}

#[test]
fn test_choreography_macro_compiles() {
    // If we reach this point, the macro compiled successfully
    // The actual types are generated at compile time
}

// Define a test trait for the effect handlers macro
trait TestEffects {
    fn get_value(&self) -> u32;
}

// Test the effect handlers macro compilation
aura_effect_handlers! {
    trait_name: TestEffects,
    mock: {
        struct_name: MockTestHandler,
        state: {
            value: u32,
        },
        methods: {
            get_value() -> u32 => {
                self.value
            },
        },
    },
    real: {
        struct_name: RealTestHandler,
        methods: {
            get_value() -> u32 => {
                42
            },
        },
    },
}

#[test]
fn test_effect_handlers_macro_compiles() {
    // Test that the effect handlers macro generated working code
    let mock = MockTestHandler::new();
    let real = RealTestHandler::new();
    
    // Test that the generated methods work
    assert_eq!(mock.get_value(), 0); // Default value
    assert_eq!(real.get_value(), 42);
}