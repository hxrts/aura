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
    use aura_choreography::{create_simple_handler, AuraRole};

    // Test Aura choreography functionality with updated handler API
    let alice_handler = create_simple_handler(AuraRole::Alice);
    assert_eq!(alice_handler.get_flow_balance(), 1000);

    // If we reach this point, the macro compiled successfully and generated working code
}

#[test]
fn test_biscuit_choreography_functionality() {
    // Test that the basic Biscuit functionality is available in generated choreography
    use aura_choreography::{create_simple_handler, map_capability_to_resource_type, AuraRole};

    // Test capability to resource type mapping
    let storage_resource_type = map_capability_to_resource_type("read_storage");
    assert_eq!(storage_resource_type, "storage");

    let relay_resource_type = map_capability_to_resource_type("initiate_request");
    assert_eq!(relay_resource_type, "relay");

    // Test handler creation
    let client_handler = create_simple_handler(AuraRole::Alice); // Use Alice since it's available
    assert_eq!(client_handler.get_flow_balance(), 1000);

    // Test that guard validation works
    let result = client_handler.validate_guard("test_capability", "storage");
    assert!(
        result.is_ok(),
        "Guard validation should work for non-empty capability"
    );
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
