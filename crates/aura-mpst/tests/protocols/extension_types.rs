//! Extension type contracts — validate that extension types can be
//! instantiated, composed, and carry the correct metadata. These types
//! form the runtime side-effect bridge for choreographic guard annotations.

use aura_core::{capability_name, types::identifiers::DeviceId, ContextId};
use aura_mpst::telltale_choreography::extensions::ExtensionRegistry;

/// Extension registry can be instantiated — basic smoke test.
#[test]
fn test_extension_registry_operations() {
    // Test that registry can be created
    let _registry = ExtensionRegistry::new();

    // The actual extension registration happens in AuraHandler::create_extension_registry()
    // which is private and tested through handler creation
}

/// Core identity types accessible through aura-mpst re-exports.
#[test]
fn test_core_types_available() {
    let _device_id = DeviceId::new_from_entropy([3u8; 32]);
    let _context_id = ContextId::new_from_entropy([0u8; 32]);
}

/// Extension types carry correct metadata and compose into a CompositeExtension
/// with the expected number of sub-extensions — the runtime guard chain
/// dispatches based on this structure.
#[test]
fn test_extension_types() {
    use aura_mpst::extensions::*;
    use aura_mpst::RoleId;

    // Test creating extension instances
    let validate_cap = ValidateCapability {
        capability: capability_name!("chat:message:send"),
        role: RoleId::new("Alice"),
    };
    assert_eq!(validate_cap.capability.as_str(), "chat:message:send");

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
        .with_capability_guard(capability_name!("chat:message:send"))
        .with_flow_cost(200)
        .with_journal_fact("operation_logged".to_string());

    assert_eq!(composite.extensions.len(), 3);
}

/// CompositeExtension preserves the guard chain ordering: capability guard
/// first, then flow cost, then journal fact. This matches the guard chain
/// sequence defined in CLAUDE.md.
#[test]
fn composite_extension_preserves_guard_chain_order() {
    use aura_mpst::extensions::*;
    use aura_mpst::RoleId;

    let composite = CompositeExtension::new(RoleId::new("Alice"), "guarded_send".to_string())
        .with_capability_guard(capability_name!("chat:message:send"))
        .with_flow_cost(50)
        .with_journal_fact("message_sent".to_string());

    assert_eq!(composite.extensions.len(), 3);

    // Verify ordering matches the guard chain: capability → cost → journal
    assert!(
        matches!(
            composite.extensions[0],
            ConcreteExtension::ValidateCapability(..)
        ),
        "first extension must be ValidateCapability"
    );
    assert!(
        matches!(
            composite.extensions[1],
            ConcreteExtension::ChargeFlowCost(..)
        ),
        "second extension must be ChargeFlowCost"
    );
    assert!(
        matches!(composite.extensions[2], ConcreteExtension::JournalFact(..)),
        "third extension must be JournalFact"
    );
}

/// Extension fields preserve their values — validates that construction
/// doesn't silently truncate or default any field.
#[test]
fn extension_fields_preserved() {
    use aura_mpst::extensions::*;
    use aura_mpst::RoleId;

    let cap = ValidateCapability {
        capability: capability_name!("recovery:coordinate"),
        role: RoleId::new("Moderator"),
    };
    assert_eq!(cap.capability.as_str(), "recovery:coordinate");
    assert_eq!(cap.role, RoleId::new("Moderator"));

    let cost = ChargeFlowCost {
        cost: u64::MAX,
        operation: "expensive_op".to_string(),
        role: RoleId::new("Spender"),
    };
    assert_eq!(cost.cost, u64::MAX, "max cost must not overflow or clamp");
    assert_eq!(cost.operation, "expensive_op");
}
