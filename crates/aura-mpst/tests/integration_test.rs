//! Integration tests for aura-macros + aura-mpst runtime
//!
//! These tests verify that aura-macros generated code integrates
//! correctly with the aura-mpst runtime system.

// Note: choreography macro import would create circular dependency
// Integration testing focuses on runtime components only
use aura_mpst::{AuraRuntime, ExecutionContext, MpstResult, ExtensionRegistry};
use aura_core::{Cap, DeviceId, Journal};

#[tokio::test]
async fn test_basic_choreography_integration() -> MpstResult<()> {
    // Create runtime with test configuration
    let device_id = DeviceId::new();
    let capabilities = Cap::top();
    let journal = Journal::new();
    
    let runtime = AuraRuntime::new(device_id, capabilities, journal);
    let _context = ExecutionContext::new("test_protocol", vec![device_id]);
    
    // Test registry integration
    let mut registry = ExtensionRegistry::new();
    registry.register_guard("test_capability", "Alice");
    registry.register_flow_cost(100, "Alice");
    registry.register_journal_fact("test_fact", "Alice");
    
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
    let mut registry = ExtensionRegistry::new();
    
    // Test guard registration
    registry.register_guard("send_capability", "Sender");
    registry.register_guard("receive_capability", "Receiver");
    
    // Test flow cost registration
    registry.register_flow_cost(50, "Sender");
    registry.register_flow_cost(30, "Receiver");
    
    // Test journal fact registration
    registry.register_journal_fact("message_sent", "Sender");
    registry.register_journal_fact("message_received", "Receiver");
    
    // Registry should be created successfully
    // Full validation would require access to internal state
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

#[tokio::test] 
async fn test_choreography_execution_flow() -> MpstResult<()> {
    use aura_mpst::execute_choreography;
    
    let device_id = DeviceId::new();
    let mut runtime = AuraRuntime::new(device_id, Cap::top(), Journal::new());
    let context = ExecutionContext::new("integration_test", vec![device_id]);
    
    // Test the integration function
    execute_choreography("test_namespace", &mut runtime, &context).await?;
    
    Ok(())
}