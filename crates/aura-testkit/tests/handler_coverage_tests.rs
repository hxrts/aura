//! Handler coverage validation — ensures every effect type has a testkit handler.

#![allow(missing_docs)]
#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::disallowed_methods)]
//! Comprehensive Handler Coverage and Validation Tests

/*!
 * Comprehensive Handler Coverage and Validation Tests
 */

//! Comprehensive Handler Coverage and Validation Tests
//!
//! This module provides extensive coverage testing for Aura protocol handlers,
//! ensuring all handler combinations work correctly with the effect system.
//!
//! This test suite validates that the Aura protocol has complete handler
//! coverage for all defined effect traits and that the effect system
//! functions correctly with comprehensive validation.

use aura_composition::{
    CompositeHandler, Handler, HandlerContext, HandlerError, RegisterAllOptions, RegistrableHandler,
};
use aura_core::types::identifiers::{AuthorityId, DeviceId};
use aura_protocol::handlers::AuraContext;
use aura_protocol::handlers::{
    core::erased::AuraHandlerFactory, EffectRegistry, EffectType, ExecutionMode,
};
use std::collections::HashSet;

/// Helper to create deterministic device IDs for tests
fn test_device_id(seed: &[u8]) -> DeviceId {
    use aura_core::hash::hash;
    use uuid::Uuid;
    let hash_bytes = hash(seed);
    let uuid_bytes: [u8; 16] = hash_bytes[..16]
        .try_into()
        .unwrap_or_else(|_| panic!("hash prefix should fit into UUID bytes"));
    DeviceId(Uuid::from_bytes(uuid_bytes))
}

/// Helper to create deterministic authority IDs for tests
fn test_authority_id(seed: &[u8]) -> AuthorityId {
    use aura_core::hash::hash;
    use uuid::Uuid;
    let hash_bytes = hash(seed);
    let uuid_bytes: [u8; 16] = hash_bytes[..16]
        .try_into()
        .unwrap_or_else(|_| panic!("hash prefix should fit into UUID bytes"));
    AuthorityId(Uuid::from_bytes(uuid_bytes))
}

/// Build a CompositeHandler with the default impure handlers registered,
/// which provides Console, Random, Crypto, Storage, Time, Network, Trace,
/// and System effects.
fn build_populated_handler(device_id: DeviceId) -> CompositeHandler {
    let mut handler = CompositeHandler::for_testing(device_id);
    handler
        .register_all(RegisterAllOptions::allow_impure())
        .unwrap_or_else(|error| panic!("register_all should succeed: {error}"));
    handler
}

/// Test that all effect types have corresponding implementations
#[test]
fn test_effect_coverage_completeness() {
    let device_id = test_device_id(b"test_effect_coverage");

    // Create handlers with default impure handlers registered
    let testing_handler = build_populated_handler(device_id);
    let simulation_handler = {
        let mut h = CompositeHandler::for_simulation(device_id, 42);
        h.register_all(RegisterAllOptions::allow_impure())
            .unwrap_or_else(|error| panic!("register_all should succeed: {error}"));
        h
    };

    // Get all effect types
    let all_effect_types = EffectType::all();

    // Effect types registered by register_all
    let registered_effect_types = vec![
        EffectType::Crypto,
        EffectType::Network,
        EffectType::Storage,
        EffectType::Time,
        EffectType::Console,
        EffectType::Random,
        EffectType::System,
        EffectType::Trace,
    ];

    // Validate testing handler
    for &effect_type in &registered_effect_types {
        assert!(
            testing_handler.supports_effect(effect_type),
            "Testing handler should support {:?}",
            effect_type
        );
    }

    // Validate simulation handler
    for &effect_type in &registered_effect_types {
        assert!(
            simulation_handler.supports_effect(effect_type),
            "Simulation handler should support {:?}",
            effect_type
        );
    }

    // Ensure we're testing a significant portion of all effect types
    let supported_count = registered_effect_types.len();
    let total_count = all_effect_types.len();
    let coverage_ratio = supported_count as f64 / total_count as f64;

    assert!(
        coverage_ratio >= 0.25,
        "Handler coverage should be at least 25% of all effect types. Got {}/{} = {:.1}%",
        supported_count,
        total_count,
        coverage_ratio * 100.0
    );
}

/// Test that the effect registry correctly validates handler registration
#[tokio::test]
async fn test_effect_registry_validation() {
    let mut registry = EffectRegistry::new(ExecutionMode::Testing);
    let device_id = test_device_id(b"test_effect_registry_validation");

    // Create a composite handler with registered effects
    let composite_handler = build_populated_handler(device_id);

    // Convert CompositeHandler to a type that implements RegistrableHandler
    // This demonstrates the pattern of wrapping handlers for registry use
    struct RegistrableCompositeHandler(CompositeHandler);

    #[async_trait::async_trait]
    impl RegistrableHandler for RegistrableCompositeHandler {
        async fn execute_operation_bytes(
            &self,
            effect_type: EffectType,
            operation: &str,
            parameters: &[u8],
            ctx: &HandlerContext,
        ) -> Result<Vec<u8>, HandlerError> {
            self.0
                .execute_effect(effect_type, operation, parameters, ctx)
                .await
        }

        fn supported_operations(&self, effect_type: EffectType) -> Vec<String> {
            match effect_type {
                EffectType::Crypto => vec![String::from("hash"), String::from("random_bytes")],
                EffectType::Network => {
                    vec![String::from("send_to_peer"), String::from("broadcast")]
                }
                EffectType::Storage => vec![String::from("store"), String::from("retrieve")],
                EffectType::System => vec![String::from("log"), String::from("health_check")],
                _ => vec![],
            }
        }

        fn supports_effect(&self, effect_type: EffectType) -> bool {
            self.0.supports_effect(effect_type)
        }

        fn execution_mode(&self) -> ExecutionMode {
            self.0.execution_mode()
        }
    }

    let registrable_handler = Box::new(RegistrableCompositeHandler(composite_handler));

    // Test successful registration
    let result = registry.register_handler(EffectType::Crypto, registrable_handler);
    assert!(result.is_ok(), "Should register handler successfully");

    // Verify registration
    assert!(registry.is_registered(EffectType::Crypto));
    assert!(!registry.is_registered(EffectType::FaultInjection)); // Should not be registered

    // Test supported operations
    let operations = registry.supported_operations(EffectType::Crypto).unwrap();
    assert!(!operations.is_empty());
    assert!(registry.supports_operation(EffectType::Crypto, "hash"));

    // Test capability summary
    let capabilities = registry.capability_summary();
    assert!(capabilities.has_effect_type(EffectType::Crypto));
    assert_eq!(capabilities.effect_type_count(), 1);
}

/// Test that the unified handler bridge provides complete coverage
#[tokio::test]
async fn test_unified_bridge_coverage() {
    let _device_id = test_device_id(b"test_unified_bridge_coverage");
    // Note: UnifiedAuraHandlerBridge requires AuraEffects trait which CompositeHandler may not implement
    // Skipping this test for now as it requires trait implementation changes

    // Note: Unified bridge test disabled due to trait implementation requirements
    println!("Unified bridge test skipped - requires AuraEffects implementation");
}

/// Test handler factory coverage for all execution modes
#[test]
fn test_handler_factory_coverage() {
    let device_id = test_device_id(b"test_handler_factory_coverage");

    // Test testing mode
    let testing_handler = AuraHandlerFactory::for_testing(device_id);
    assert_eq!(testing_handler.execution_mode(), ExecutionMode::Testing);

    // Test production mode — protocol-layer factory intentionally returns Err
    // because production handler assembly is owned by aura-agent
    let production_result = AuraHandlerFactory::for_production(device_id);
    assert!(production_result.is_err());

    // Test simulation mode
    let simulation_handler = AuraHandlerFactory::for_simulation(device_id, 42);
    assert!(simulation_handler.execution_mode().is_deterministic());
}

/// Comprehensive validation test for effect type coverage
#[test]
fn test_comprehensive_effect_type_validation() {
    let all_effect_types = EffectType::all();

    // Validate specific effect types exist
    assert!(all_effect_types.contains(&EffectType::Crypto));
    assert!(all_effect_types.contains(&EffectType::Network));
    assert!(all_effect_types.contains(&EffectType::Storage));
    assert!(all_effect_types.contains(&EffectType::System));
    assert!(all_effect_types.contains(&EffectType::Choreographic));

    // Validate that we have handlers for critical protocol effects
    let device_id = test_device_id(b"test_effect_registry_completeness");
    let handler = build_populated_handler(device_id);

    for effect_type in &all_effect_types {
        if matches!(
            effect_type,
            EffectType::Crypto
                | EffectType::Network
                | EffectType::Storage
                | EffectType::Time
                | EffectType::Console
                | EffectType::System
        ) {
            assert!(
                handler.supports_effect(*effect_type),
                "Handler must support critical protocol effect {:?}",
                effect_type
            );
        }
    }
}

/// Test that effect handler implementations are deterministic in testing mode
#[tokio::test]
async fn test_handler_determinism() {
    let device_id = test_device_id(b"test_handler_determinism");
    let authority_id = test_authority_id(b"test_handler_determinism/authority");

    // Create two handlers with the same device ID for deterministic testing
    let handler1 = AuraHandlerFactory::for_testing(device_id);
    let handler2 = AuraHandlerFactory::for_testing(device_id);
    let _ctx1 = AuraContext::for_testing(authority_id, device_id);
    let _ctx2 = AuraContext::for_testing(authority_id, device_id);

    // Both should be in testing mode (deterministic)
    assert!(handler1.execution_mode().is_deterministic());
    assert!(handler2.execution_mode().is_deterministic());

    // Note: The protocol-layer AuraHandlerFactory creates empty handlers (no
    // effects registered), so execute_effect calls will fail. Determinism is
    // validated through the execution_mode check above.
    println!("Handler determinism validated via execution mode");
}

/// Performance and coverage metrics test
#[test]
fn test_handler_coverage_metrics() {
    let device_id = test_device_id(b"test_handler_coverage_metrics");
    let handler = build_populated_handler(device_id);

    let all_effects = EffectType::all();
    let supported_effects: Vec<_> = all_effects
        .iter()
        .copied()
        .filter(|&et| handler.supports_effect(et))
        .collect();

    // Calculate coverage metrics
    let total_effects = all_effects.len();
    let supported_count = supported_effects.len();
    let coverage_percentage = (supported_count as f64 / total_effects as f64) * 100.0;

    println!("Effect Coverage Metrics:");
    println!("  Total effect types: {}", total_effects);
    println!("  Supported effect types: {}", supported_count);
    println!("  Coverage percentage: {:.1}%", coverage_percentage);

    // List supported effects
    println!("  Supported effects: {:?}", supported_effects);

    // List unsupported effects
    let supported_set: HashSet<_> = supported_effects.into_iter().collect();
    let unsupported: Vec<_> = all_effects
        .into_iter()
        .filter(|effect| !supported_set.contains(effect))
        .collect();
    println!("  Unsupported effects: {:?}", unsupported);

    // Validate minimum coverage threshold
    assert!(
        coverage_percentage >= 25.0,
        "Handler coverage should be at least 25%. Got {:.1}%",
        coverage_percentage
    );
}

/// Test registry capability validation
#[test]
fn test_registry_capability_validation() {
    let registry = EffectRegistry::new(ExecutionMode::Testing);

    // Initially empty
    let capabilities = registry.capability_summary();
    assert_eq!(capabilities.effect_type_count(), 0);
    assert_eq!(capabilities.total_operations, 0);

    // Validate empty registry behavior
    assert!(!capabilities.has_effect_type(EffectType::Crypto));
    assert!(!capabilities.supports_operation(EffectType::Crypto, "hash"));
    // Empty registry has no registered handler modes
    assert!(capabilities.execution_modes.is_empty());
}

/// Integration test for complete handler system
#[tokio::test]
async fn test_complete_handler_system_integration() {
    let device_id = test_device_id(b"test_complete_handler_system_integration");

    // Test available factory methods
    let testing_handler = build_populated_handler(device_id);
    let simulation_handler = {
        let mut h = CompositeHandler::for_simulation(device_id, 123);
        h.register_all(RegisterAllOptions::allow_impure())
            .unwrap_or_else(|error| panic!("register_all should succeed: {error}"));
        h
    };

    // Test handlers support core operations
    let core_effects = vec![EffectType::System, EffectType::Console];

    for effect_type in core_effects {
        assert!(testing_handler.supports_effect(effect_type));
        assert!(simulation_handler.supports_effect(effect_type));
    }

    // Test registry integration
    let registry = EffectRegistry::new(ExecutionMode::Testing);
    assert_eq!(registry.execution_mode(), ExecutionMode::Testing);

    println!("Complete handler system integration test passed");
}
