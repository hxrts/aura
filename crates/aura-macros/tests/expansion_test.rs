//! Macro expansion test - verifies the macro generates valid code
//!
//! This test uses cargo-expand to verify the macro output compiles.

use aura_macros::AuraMiddleware;

// Simple test case that should expand correctly
#[derive(AuraMiddleware)]
#[middleware(effects = "[NetworkEffects]")]
struct TestMiddleware<H> {
    inner: H,
}

// Compile-time verification that the macro generates valid code
#[test]
fn test_macro_expands() {
    // This test passes if the file compiles
    assert!(true);
}
