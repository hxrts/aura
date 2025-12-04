//! Integration tests for aura-macros
//!
//! These tests verify that the proc macros compile and generate valid code.

use aura_macros::aura_effect_handlers;

// Choreography macro is tested via production usage in crates like:
// - aura-authenticate/src/session_creation.rs
// - aura-rendezvous/src/protocol.rs
// See those files for working examples.

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
