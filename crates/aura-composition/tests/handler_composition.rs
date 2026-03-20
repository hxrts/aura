//! Handler composition integration tests.
//!
//! Exercises the public API: build composite handlers, register effects,
//! query capabilities across the assembled system.

use aura_composition::registry::RegistrableHandler;
use aura_composition::{CompositeHandler, CompositeHandlerAdapter, RegisterAllOptions};
use aura_core::types::identifiers::DeviceId;
use aura_core::EffectType;

fn test_device() -> DeviceId {
    DeviceId::new_from_entropy([1u8; 32])
}

/// Full composition: build handler, register all impure effects, verify
/// capabilities across the assembled system.
#[test]
fn full_composition_produces_working_system() {
    let device_id = test_device();
    let mut handler = CompositeHandler::for_testing(device_id);

    handler
        .register_all(RegisterAllOptions::allow_impure())
        .unwrap_or_else(|error| panic!("register_all should succeed: {error}"));

    // Core effect types should all be supported
    assert!(handler.has_handler(EffectType::Crypto));
    assert!(handler.has_handler(EffectType::Storage));
    assert!(handler.has_handler(EffectType::Network));
    assert!(handler.has_handler(EffectType::Time));
    assert!(handler.has_handler(EffectType::Console));
    assert!(handler.has_handler(EffectType::Random));
}

/// Adapter wraps the composite handler and preserves capabilities.
#[test]
fn adapter_preserves_effect_support() {
    let device_id = test_device();
    let adapter = CompositeHandlerAdapter::for_testing(device_id);

    // Adapter should report supported operations for known types
    let console_ops = adapter.supported_operations(EffectType::Console);
    assert!(!console_ops.is_empty(), "Console should have operations");

    let random_ops = adapter.supported_operations(EffectType::Random);
    assert!(!random_ops.is_empty(), "Random should have operations");
}

/// Register_all without allow_impure fails.
#[test]
fn register_all_requires_impure_opt_in() {
    let device_id = test_device();
    let mut handler = CompositeHandler::for_testing(device_id);

    let result = handler.register_all(RegisterAllOptions::default());
    assert!(
        result.is_err(),
        "register_all must require explicit allow_impure opt-in"
    );
}
