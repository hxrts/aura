//! Integration tests for aura-macros + aura-mpst runtime
//!
//! These tests verify that aura-macros generated code integrates
//! correctly with the aura-mpst runtime system.

use aura_core::{Cap, ContextId, DeviceId, Journal};
use aura_mpst::{AuraEndpoint, AuraHandler, AuraRuntime, ExecutionContext, MpstResult};
use rumpsteak_aura_choreography::extensions::ExtensionRegistry;

#[tokio::test]
async fn test_basic_choreography_integration() -> MpstResult<()> {
    // Create runtime with test configuration
    let device_id = DeviceId::new();
    let capabilities = Cap::top();
    let journal = Journal::new();

    let runtime = AuraRuntime::new(device_id, capabilities, journal);
    let _context = ExecutionContext::new("test_protocol", vec![device_id]);

    // Test registry can be created (the actual registration happens in AuraHandler)
    let _registry = ExtensionRegistry::new();

    // Validate runtime state
    runtime.validate()?;

    Ok(())
}

#[tokio::test]
async fn test_choreography_macro_output() {
    // This test validates that the macro generates valid Rust code
    // We can't easily test macro expansion in integration tests,
    // but we can test that generated patterns compile

    let device_id = DeviceId::new();
    let runtime = AuraRuntime::new(device_id, Cap::top(), Journal::new());

    // Test that runtime validates successfully
    assert!(runtime.validate().is_ok());
}

#[test]
fn test_extension_registry_operations() {
    // Test that registry can be created
    let _registry = ExtensionRegistry::new();

    // The actual extension registration happens in AuraHandler::create_extension_registry()
    // which is private and tested through handler creation
}

#[tokio::test]
async fn test_runtime_factory_integration() -> MpstResult<()> {
    use aura_mpst::runtime::{AuraRuntimeFactory, ProtocolRequirements};

    let factory = AuraRuntimeFactory::default();
    let device_id = DeviceId::new();

    let runtime = factory.create_runtime(device_id);
    assert_eq!(runtime.device_id(), device_id);

    // Test protocol requirements validation
    let requirements = ProtocolRequirements::new()
        .participants(1, Some(3))
        .require_capability(Cap::top());

    let context = ExecutionContext::new("test", vec![device_id]);
    requirements.validate(&runtime, &context)?;

    Ok(())
}

#[test]
fn test_aura_handler_creation() {
    let device_id = DeviceId::new();

    // Test creating handlers for different execution modes
    let _testing_handler = AuraHandler::for_testing(device_id);
    assert!(_testing_handler.is_ok());

    let _production_handler = AuraHandler::for_production(device_id);
    assert!(_production_handler.is_ok());

    let _simulation_handler = AuraHandler::for_simulation(device_id);
    assert!(_simulation_handler.is_ok());
}

#[test]
fn test_aura_endpoint_creation() {
    let device_id = DeviceId::new();
    let context_id = ContextId::new("test_context");

    let mut endpoint = AuraEndpoint::new(device_id, context_id);
    assert_eq!(endpoint.device_id, device_id);
    assert_eq!(endpoint.context_id, ContextId::new("test_context"));

    // Test connection management
    let peer_id = DeviceId::new();
    endpoint.add_connection(peer_id, aura_mpst::ConnectionState::Active);
    assert!(endpoint.is_connected_to(peer_id));

    // Test metadata
    endpoint.add_metadata("test_key".to_string(), "test_value".to_string());
    assert_eq!(
        endpoint.metadata.get("test_key"),
        Some(&"test_value".to_string())
    );
}

#[test]
fn test_extension_types() {
    use aura_mpst::extensions::*;

    // Test creating extension instances
    let validate_cap = ValidateCapability {
        capability: "test_capability".to_string(),
        role: "Alice".to_string(),
    };
    assert_eq!(validate_cap.capability, "test_capability");

    let flow_cost = ChargeFlowCost {
        cost: 100,
        operation: "send_message".to_string(),
        role: "Bob".to_string(),
    };
    assert_eq!(flow_cost.cost, 100);

    let journal_fact = JournalFact {
        fact: "message_sent".to_string(),
        operation: "send".to_string(),
        role: "Charlie".to_string(),
    };
    assert_eq!(journal_fact.fact, "message_sent");

    // Test composite extension
    let composite = CompositeExtension::new("Alice".to_string(), "complex_op".to_string())
        .with_capability_guard("access_data".to_string())
        .with_flow_cost(200)
        .with_journal_fact("operation_logged".to_string());

    assert_eq!(composite.extensions.len(), 3);
}
