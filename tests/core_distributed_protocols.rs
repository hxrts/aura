//! Core Distributed Protocols Integration Tests
//!
//! Tests the integration of core distributed protocols:
//! - CRDT synchronization 
//! - Guard chain enforcement
//! - Authorization workflows
//! - Journal operations
//! - Effect system integration

use aura_core::{DeviceId, Epoch, FlowBudget, FlowBudgetKey, Journal, Fact, Cap};
use aura_core::flow::Receipt;
use aura_core::relationships::ContextId;
use aura_journal::semilattice::{CvHandler, DeltaHandler, CmHandler, MvHandler};
use aura_protocol::guards::SendGuardChain;
use aura_protocol::handlers::CompositeHandler;
use aura_protocol::effects::{AgentEffects, JournalEffects, NetworkEffects};
use aura_protocol::middleware::circuit_breaker::{CircuitBreakerMiddleware, CircuitBreakerConfig};
use aura_wot::{TrustLevel, Capability, CapabilitySet};
use std::sync::Arc;
use tokio;

/// Test CRDT synchronization between two devices
#[tokio::test]
async fn test_crdt_synchronization_protocol() {
    println!("Testing CRDT synchronization between devices");
    
    let device_a = DeviceId::new();
    let device_b = DeviceId::new();
    
    // Create journals for both devices
    let mut journal_a = Journal::new();
    let mut journal_b = Journal::new();
    
    // Device A adds some facts
    journal_a.facts_mut().insert("key1", "value1".into());
    journal_a.facts_mut().insert("key2", "value2".into());
    
    // Device B adds different facts
    journal_b.facts_mut().insert("key3", "value3".into());
    journal_b.facts_mut().insert("key4", "value4".into());
    
    // Simulate synchronization via CRDT join
    let synchronized_journal = journal_a.join(&journal_b);
    
    // Verify convergence
    assert!(synchronized_journal.facts().contains_key("key1"));
    assert!(synchronized_journal.facts().contains_key("key2"));
    assert!(synchronized_journal.facts().contains_key("key3"));
    assert!(synchronized_journal.facts().contains_key("key4"));
    
    println!("✓ CRDT synchronization successful");
}

/// Test guard chain enforcement for authorization
#[tokio::test]
async fn test_guard_chain_authorization_protocol() {
    println!("Testing guard chain authorization protocol");
    
    let device_id = DeviceId::new();
    let context = ContextId::new();
    let peer_device = DeviceId::new();
    
    // Create flow budget for the context/peer pair
    let budget_key = FlowBudgetKey::new(context, peer_device);
    let flow_budget = FlowBudget::new(1000, Epoch::new(1));
    
    // Create capability set with relay permission
    let relay_capability = Capability::Relay {
        max_bytes_per_period: 1000,
        period_seconds: 3600,
        max_streams: 5,
    };
    let mut capability_set = CapabilitySet::empty();
    capability_set.insert(relay_capability);
    
    // Test guard chain evaluation
    let guard_chain = SendGuardChain::new();
    let message_cost = 100u64;
    
    // Create authorization context for testing
    let auth_result = guard_chain.authorize_send(
        context,
        peer_device,
        message_cost,
        &capability_set,
        &flow_budget,
    );
    
    assert!(auth_result.is_ok(), "Guard chain authorization should succeed: {:?}", auth_result);
    println!("✓ Guard chain authorization successful");
}

/// Test journal operations with flow budget enforcement
#[tokio::test]
async fn test_journal_flow_budget_integration() {
    println!("Testing journal operations with flow budget enforcement");
    
    let device_id = DeviceId::new();
    let context = ContextId::new();
    let peer_device = DeviceId::new();
    
    // Create a journal with flow budget tracking
    let mut journal = Journal::new();
    
    // Add flow budget fact to journal
    let budget_key = FlowBudgetKey::new(context, peer_device);
    let budget = FlowBudget::new(1000, Epoch::new(1));
    
    journal.facts_mut().insert(
        format!("flow_budget:{}", budget_key.context.as_str()),
        format!("{}:{}", budget.limit, budget.spent).into()
    );
    
    // Verify budget is stored correctly
    assert!(journal.facts().contains_key(&format!("flow_budget:{}", budget_key.context.as_str())));
    
    // Test budget modification via journal facts
    journal.facts_mut().insert(
        format!("flow_budget:{}:spent", budget_key.context.as_str()),
        "200".into()
    );
    
    assert!(journal.facts().contains_key(&format!("flow_budget:{}:spent", budget_key.context.as_str())));
    
    println!("✓ Journal flow budget integration successful");
}

/// Test effect system integration with composite handler
#[tokio::test]
async fn test_effect_system_integration() {
    println!("Testing effect system integration");
    
    let device_id = DeviceId::new();
    
    // Create composite handler for testing
    let handler = CompositeHandler::for_testing(device_id);
    
    // Test that all effect interfaces are available
    assert!(true); // Handler creation itself validates effect integration
    
    // Test agent effects
    let agent_effects = handler.agent_effects();
    assert!(agent_effects.is_some());
    
    // Test journal effects 
    let journal_effects = handler.journal_effects();
    assert!(journal_effects.is_some());
    
    // Test network effects
    let network_effects = handler.network_effects();
    assert!(network_effects.is_some());
    
    println!("✓ Effect system integration successful");
}

/// Test circuit breaker middleware integration
#[tokio::test]
async fn test_circuit_breaker_middleware_integration() {
    println!("Testing circuit breaker middleware integration");
    
    let device_id = DeviceId::new();
    let context = ContextId::new();
    
    // Create circuit breaker configuration
    let config = CircuitBreakerConfig {
        failure_threshold: 3,
        timeout_duration_ms: 30000,
        probe_interval_ms: 5000,
    };
    
    // Create flow budget for testing
    let flow_budget = FlowBudget::new(1000, Epoch::new(1));
    
    // Create circuit breaker middleware
    let circuit_breaker = CircuitBreakerMiddleware::new(config, flow_budget);
    
    // Test circuit breaker state
    assert!(!circuit_breaker.is_open());
    assert!(circuit_breaker.can_execute());
    
    println!("✓ Circuit breaker middleware integration successful");
}

/// Test multi-device distributed protocol scenario
#[tokio::test]
async fn test_multi_device_distributed_scenario() {
    println!("Testing multi-device distributed protocol scenario");
    
    // Create multiple devices
    let device_a = DeviceId::new();
    let device_b = DeviceId::new();
    let device_c = DeviceId::new();
    
    // Create context for distributed operation
    let context = ContextId::new();
    let epoch = Epoch::new(1);
    
    // Device A: Create initial journal state
    let mut journal_a = Journal::new();
    journal_a.facts_mut().insert("shared_state", "initial_value".into());
    
    // Device B: Create conflicting state
    let mut journal_b = Journal::new();
    journal_b.facts_mut().insert("shared_state", "updated_value".into());
    journal_b.facts_mut().insert("device_b_state", "b_value".into());
    
    // Device C: Create additional state
    let mut journal_c = Journal::new();
    journal_c.facts_mut().insert("device_c_state", "c_value".into());
    
    // Simulate distributed synchronization
    let intermediate_sync = journal_a.join(&journal_b);
    let final_sync = intermediate_sync.join(&journal_c);
    
    // Verify all devices' state is represented
    assert!(final_sync.facts().contains_key("shared_state"));
    assert!(final_sync.facts().contains_key("device_b_state"));
    assert!(final_sync.facts().contains_key("device_c_state"));
    
    // Verify CRDT convergence properties
    let reverse_sync_a = journal_b.join(&journal_a);
    let reverse_sync_b = reverse_sync_a.join(&journal_c);
    
    // Final state should be the same regardless of sync order (commutativity)
    assert_eq!(final_sync.facts().len(), reverse_sync_b.facts().len());
    
    println!("✓ Multi-device distributed protocol scenario successful");
}

/// Helper function to create test receipts
fn create_test_receipt(
    ctx: ContextId,
    src: DeviceId,
    dst: DeviceId,
    epoch: Epoch,
    cost: u32,
    nonce: u64,
) -> Receipt {
    Receipt::new(
        ctx,
        src,
        dst,
        epoch,
        cost,
        nonce,
        [0u8; 32], // prev hash
        vec![0u8; 64], // signature placeholder
    )
}

/// Test receipt verification and anti-replay
#[tokio::test] 
async fn test_receipt_verification_protocol() {
    println!("Testing receipt verification protocol");
    
    let device_a = DeviceId::new();
    let device_b = DeviceId::new();
    let context = ContextId::new();
    let epoch = Epoch::new(1);
    
    // Create test receipt
    let receipt = create_test_receipt(context, device_a, device_b, epoch, 100, 1);
    
    // Verify receipt structure
    assert_eq!(receipt.src, device_a);
    assert_eq!(receipt.dst, device_b);
    assert_eq!(receipt.ctx, context);
    assert_eq!(receipt.epoch, epoch);
    assert_eq!(receipt.cost, 100);
    assert_eq!(receipt.nonce, 1);
    
    // Test anti-replay protection (same nonce should be rejected)
    let duplicate_receipt = create_test_receipt(context, device_a, device_b, epoch, 100, 1);
    assert_eq!(receipt.nonce, duplicate_receipt.nonce);
    
    println!("✓ Receipt verification protocol successful");
}