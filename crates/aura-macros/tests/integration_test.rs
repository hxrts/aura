//! Integration tests for aura-macros
//!
//! These tests verify that the proc macros compile and generate valid code.

use aura_macros::{aura_effect_handlers, choreography};
use serde::{Deserialize, Serialize};

// Message types for choreography (matching the expected Ping/Pong pattern)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ping {
    pub data: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pong {
    pub data: String,
}

// Test that the macro compiles and generates valid types
// This generates both rumpsteak session types and Aura choreography wrapper
choreography! {
    choreography TestProtocol {
        roles: Alice, Bob;
        Alice -> Bob: Ping;
        Bob -> Alice: Pong;
    }
}

#[test]
fn test_choreography_macro_compiles() {
    // Test that both Aura choreography module and rumpsteak session types were generated
    use aura_choreography::{create_handler, AuraRole};

    // Test Aura choreography functionality
    let alice_handler = create_handler(AuraRole::Alice, vec!["test".to_string()]);
    assert_eq!(alice_handler.get_flow_balance(), 1000);

    // If we reach this point, the macro compiled successfully and generated working code
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
