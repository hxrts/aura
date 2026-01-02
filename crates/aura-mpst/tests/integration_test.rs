//! Integration tests for aura-macros + aura-mpst runtime
//!
//! These tests verify that aura-macros generated code integrates
//! correctly with the aura-mpst runtime system.
//!
//! Note: Tests for deprecated AuraRuntime have been removed.
//! Use aura-agent::AgentRuntime for new code.

use aura_core::{identifiers::DeviceId, ContextId};
use rumpsteak_aura_choreography::extensions::ExtensionRegistry;

#[test]
fn test_extension_registry_operations() {
    // Test that registry can be created
    let _registry = ExtensionRegistry::new();

    // The actual extension registration happens in AuraHandler::create_extension_registry()
    // which is private and tested through handler creation
}

#[test]
fn test_core_types_available() {
    let _device_id = DeviceId::new_from_entropy([3u8; 32]);
    let _context_id = ContextId::new_from_entropy([0u8; 32]);
}

#[test]
fn test_extension_types() {
    use aura_mpst::extensions::*;
    use aura_mpst::RoleId;

    // Test creating extension instances
    let validate_cap = ValidateCapability {
        capability: "test_capability".to_string(),
        role: RoleId::new("Alice"),
    };
    assert_eq!(validate_cap.capability, "test_capability");

    let flow_cost = ChargeFlowCost {
        cost: 100,
        operation: "send_message".to_string(),
        role: RoleId::new("Bob"),
    };
    assert_eq!(flow_cost.cost, 100);

    let journal_fact = JournalFact {
        fact: "message_sent".to_string(),
        operation: "send".to_string(),
        role: RoleId::new("Carol"),
    };
    assert_eq!(journal_fact.fact, "message_sent");

    // Test composite extension
    let composite = CompositeExtension::new(RoleId::new("Alice"), "complex_op".to_string())
        .with_capability_guard("access_data".to_string())
        .with_flow_cost(200)
        .with_journal_fact("operation_logged".to_string());

    assert_eq!(composite.extensions.len(), 3);
}
