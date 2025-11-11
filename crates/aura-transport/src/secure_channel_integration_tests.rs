//! Comprehensive integration tests for SecureChannel management system
//!
//! These tests verify the complete SecureChannel implementation including:
//! - Registry invariants enforcement
//! - Teardown trigger handling
//! - Reconnection behavior
//! - Integration with NetworkTransport
//!
//! As specified in work/007.md task #6

use crate::{
    network::{NetworkConfig, NetworkTransport},
    reconnect::{ReconnectConfig, ReconnectCoordinator, ReconnectResult},
    secure_channel::{
        ChannelKey, ChannelStatus, RegistryConfig, SecureChannelRegistry, TeardownReason,
    },
};
use aura_core::{
    flow::FlowBudget, relationships::ContextId, session_epochs::Epoch, AuraError, DeviceId,
};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

/// Test fixture for SecureChannel integration tests
struct SecureChannelTestFixture {
    registry: Arc<SecureChannelRegistry>,
    reconnect_coordinator: ReconnectCoordinator,
    network_transport: NetworkTransport,
    device_id: DeviceId,
    peer_device: DeviceId,
    context: ContextId,
    epoch: Epoch,
    flow_budget: FlowBudget,
}

impl SecureChannelTestFixture {
    async fn new() -> Self {
        let device_id = DeviceId::new();
        let peer_device = DeviceId::new();
        let context = ContextId::new("test_context");
        let epoch = Epoch::new(1);
        let flow_budget = FlowBudget::new(1000, epoch);

        let registry = Arc::new(SecureChannelRegistry::new(RegistryConfig::default()));
        let reconnect_coordinator = ReconnectCoordinator::with_defaults(registry.clone(), epoch);
        let network_transport = NetworkTransport::new_with_registry(
            device_id,
            NetworkConfig::default(),
            registry.clone(),
        );

        Self {
            registry,
            reconnect_coordinator,
            network_transport,
            device_id,
            peer_device,
            context,
            epoch,
            flow_budget,
        }
    }

    async fn create_channel(&self) -> Result<(), AuraError> {
        self.registry
            .get_or_create_channel(self.context, self.peer_device, self.epoch, self.flow_budget)
            .await
    }

    async fn channel_exists(&self) -> bool {
        self.registry.has_channel(self.context, self.peer_device).await
    }

    async fn channel_is_active(&self) -> bool {
        self.registry
            .get_active_channel(self.context, self.peer_device)
            .await
            .is_some()
    }
}

#[tokio::test]
async fn test_one_channel_per_context_peer_invariant() {
    let fixture = SecureChannelTestFixture::new().await;

    // Create channel multiple times - should not create duplicates
    for _ in 0..3 {
        fixture.create_channel().await.unwrap();
    }

    let stats = fixture.registry.get_registry_stats().await;
    assert_eq!(stats.total_channels, 1, "Should have exactly one channel");
    assert_eq!(stats.establishing_channels, 1, "Channel should be establishing");

    // Validate registry invariants
    let violations = fixture.registry.validate_invariants().await.unwrap();
    assert!(
        violations.is_empty(),
        "Registry invariants violated: {:?}",
        violations
    );
}

#[tokio::test]
async fn test_epoch_rotation_teardown_trigger() {
    let fixture = SecureChannelTestFixture::new().await;
    fixture.create_channel().await.unwrap();

    // Verify channel exists
    assert!(fixture.channel_exists().await);

    // Trigger epoch rotation
    let new_epoch = Epoch::new(2);
    fixture.registry.trigger_epoch_rotation(new_epoch).await;

    // Process teardown queue
    let teardown_count = fixture.registry.process_teardown_queue().await.unwrap();
    assert_eq!(teardown_count, 1, "Should have torn down one channel");

    // Verify channel state after teardown
    let stats = fixture.registry.get_registry_stats().await;
    assert_eq!(stats.terminated_channels, 1, "Should have one terminated channel");
}

#[tokio::test]
async fn test_capability_shrink_teardown_trigger() {
    let fixture = SecureChannelTestFixture::new().await;
    fixture.create_channel().await.unwrap();

    // Create a flow budget with significantly smaller limit (< 3/4 of original)
    let shrunk_budget = FlowBudget::new(500, fixture.epoch); // 500 < 750 (3/4 of 1000)

    // Trigger capability shrink
    fixture
        .registry
        .trigger_capability_shrink(fixture.context, fixture.peer_device, shrunk_budget)
        .await
        .unwrap();

    // Process teardown queue
    let teardown_count = fixture.registry.process_teardown_queue().await.unwrap();
    assert_eq!(teardown_count, 1, "Should have torn down one channel due to capability shrink");
}

#[tokio::test]
async fn test_context_invalidation_teardown_trigger() {
    let fixture = SecureChannelTestFixture::new().await;

    // Create multiple channels in the same context
    fixture.create_channel().await.unwrap();

    let another_peer = DeviceId::new();
    let another_budget = FlowBudget::new(1000, fixture.epoch);
    fixture
        .registry
        .get_or_create_channel(fixture.context, another_peer, fixture.epoch, another_budget)
        .await
        .unwrap();

    // Verify both channels exist
    let initial_stats = fixture.registry.get_registry_stats().await;
    assert_eq!(initial_stats.total_channels, 2);

    // Trigger context invalidation
    let invalidation_reason = "Test context invalidation".to_string();
    fixture
        .registry
        .trigger_context_invalidation(fixture.context, invalidation_reason)
        .await;

    // Process teardown queue
    let teardown_count = fixture.registry.process_teardown_queue().await.unwrap();
    assert_eq!(teardown_count, 2, "Should have torn down both channels in context");

    // Verify all channels in context are terminated
    let final_stats = fixture.registry.get_registry_stats().await;
    assert_eq!(final_stats.terminated_channels, 2);
}

#[tokio::test]
async fn test_registry_capacity_limits() {
    let config = RegistryConfig {
        max_channels: 2, // Very low limit for testing
        ..Default::default()
    };
    let registry = Arc::new(SecureChannelRegistry::new(config));
    let context = ContextId::new("test_context");
    let epoch = Epoch::new(1);
    let budget = FlowBudget::new(1000, epoch);

    // Create channels up to limit
    for i in 0..2 {
        let peer = DeviceId::new();
        registry
            .get_or_create_channel(context, peer, epoch, budget)
            .await
            .unwrap();
    }

    // Verify at capacity
    let stats = registry.get_registry_stats().await;
    assert_eq!(stats.total_channels, 2);

    // Attempt to exceed capacity
    let excess_peer = DeviceId::new();
    let result = registry
        .get_or_create_channel(context, excess_peer, epoch, budget)
        .await;

    assert!(result.is_err(), "Should fail when exceeding capacity");
    assert!(
        result.unwrap_err().to_string().contains("at capacity"),
        "Error should mention capacity limit"
    );
}

#[tokio::test]
async fn test_channel_cleanup_and_lifecycle() {
    let fixture = SecureChannelTestFixture::new().await;
    fixture.create_channel().await.unwrap();

    // Trigger teardown
    fixture.registry.trigger_epoch_rotation(Epoch::new(2)).await;
    fixture.registry.process_teardown_queue().await.unwrap();

    // Before cleanup
    let stats_before = fixture.registry.get_registry_stats().await;
    assert_eq!(stats_before.total_channels, 1);
    assert_eq!(stats_before.terminated_channels, 1);

    // Cleanup terminated channels
    let cleaned_count = fixture.registry.cleanup_terminated_channels().await;
    assert_eq!(cleaned_count, 1);

    // After cleanup
    let stats_after = fixture.registry.get_registry_stats().await;
    assert_eq!(stats_after.total_channels, 0);
}

#[tokio::test]
async fn test_reconnect_coordinator_integration() {
    let fixture = SecureChannelTestFixture::new().await;
    fixture.create_channel().await.unwrap();

    // Schedule a reconnect attempt
    let teardown_reason = TeardownReason::EpochRotation {
        old_epoch: fixture.epoch,
        new_epoch: Epoch::new(2),
    };

    fixture
        .reconnect_coordinator
        .schedule_reconnect(
            ChannelKey::new(fixture.context, fixture.peer_device),
            teardown_reason,
            Epoch::new(2),
            fixture.flow_budget,
        )
        .await
        .unwrap();

    // Verify reconnect was scheduled
    let stats = fixture.reconnect_coordinator.get_reconnect_stats().await;
    assert_eq!(stats.pending_attempts, 1);

    // Update coordinator epoch to allow reconnection
    fixture
        .reconnect_coordinator
        .update_epoch(Epoch::new(2))
        .await;

    // Process reconnections
    let results = fixture
        .reconnect_coordinator
        .process_reconnections()
        .await
        .unwrap();

    assert_eq!(results.len(), 1);
    match &results[0] {
        ReconnectResult::Success { .. } => {
            // Reconnection should succeed in test environment
        }
        other => panic!("Expected successful reconnection, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_epoch_boundary_enforcement() {
    let config = ReconnectConfig {
        require_epoch_advancement: true,
        ..Default::default()
    };
    let registry = Arc::new(SecureChannelRegistry::with_defaults());
    let coordinator = ReconnectCoordinator::new(registry, config, Epoch::new(5));

    let context = ContextId::new("test_context");
    let peer = DeviceId::new();
    let channel_key = ChannelKey::new(context, peer);

    // Try to schedule reconnect with old epoch
    let old_epoch = Epoch::new(3);
    let budget = FlowBudget::new(1000, old_epoch);
    let reason = TeardownReason::Manual;

    coordinator
        .schedule_reconnect(channel_key, reason, old_epoch, budget)
        .await
        .unwrap();

    // Should not be scheduled due to epoch boundary violation
    let stats = coordinator.get_reconnect_stats().await;
    assert_eq!(stats.pending_attempts, 0);
}

#[tokio::test]
async fn test_network_transport_integration() {
    let fixture = SecureChannelTestFixture::new().await;

    // Use NetworkTransport to add peer with context
    let peer_addr = "192.168.1.100:8080".parse().unwrap();
    fixture
        .network_transport
        .add_peer_for_context(
            fixture.context,
            fixture.peer_device,
            peer_addr,
            fixture.epoch,
            fixture.flow_budget,
        )
        .await
        .unwrap();

    // Verify channel was created in registry
    assert!(fixture.channel_exists().await);

    // Test legacy method (should warn but work)
    let legacy_peer = DeviceId::new();
    fixture
        .network_transport
        .add_peer(legacy_peer, peer_addr)
        .await
        .unwrap();

    // Should have created another channel
    let stats = fixture
        .network_transport
        .channel_registry()
        .get_registry_stats()
        .await;
    assert_eq!(stats.total_channels, 2);
}

#[tokio::test]
async fn test_multiple_contexts_isolation() {
    let registry: Arc<SecureChannelRegistry> = Arc::new(SecureChannelRegistry::with_defaults());
    let peer = DeviceId::new();
    let epoch = Epoch::new(1);
    let budget = FlowBudget::new(1000, epoch);

    // Create channels in different contexts for same peer
    let context1 = ContextId::new("context1");
    let context2 = ContextId::new("context2");

    registry
        .get_or_create_channel(context1, peer, epoch, budget)
        .await
        .unwrap();
    registry
        .get_or_create_channel(context2, peer, epoch, budget)
        .await
        .unwrap();

    // Should have two separate channels
    let stats = registry.get_registry_stats().await;
    assert_eq!(stats.total_channels, 2);
    assert_eq!(stats.active_context_count(), 2);
    assert_eq!(stats.active_peer_count(), 1);

    // Invalidate one context
    registry
        .trigger_context_invalidation(context1, "Test invalidation".to_string())
        .await;
    registry.process_teardown_queue().await.unwrap();

    // Should still have one active channel
    let final_stats = registry.get_registry_stats().await;
    assert_eq!(final_stats.total_channels, 2);
    assert_eq!(final_stats.terminated_channels, 1);
    assert_eq!(final_stats.establishing_channels, 1);
}

#[tokio::test]
async fn test_concurrent_operations() {
    let registry = Arc::new(SecureChannelRegistry::with_defaults());
    let context = ContextId::new("concurrent_test");
    let epoch = Epoch::new(1);

    // Create multiple channels concurrently
    let mut handles = Vec::new();
    for i in 0..10 {
        let registry_clone = registry.clone();
        let peer = DeviceId::new();
        let budget = FlowBudget::new(1000, epoch);

        let handle = tokio::spawn(async move {
            registry_clone
                .get_or_create_channel(context, peer, epoch, budget)
                .await
        });
        handles.push(handle);

        // Small delay to encourage interleaving
        sleep(Duration::from_millis(1)).await;
    }

    // Wait for all to complete
    for handle in handles {
        handle.await.unwrap().unwrap();
    }

    // Verify all channels were created
    let stats = registry.get_registry_stats().await;
    assert_eq!(stats.total_channels, 10);

    // Trigger concurrent teardowns
    registry.trigger_context_invalidation(context, "Cleanup".to_string()).await;
    let teardown_count = registry.process_teardown_queue().await.unwrap();
    assert_eq!(teardown_count, 10);

    // Validate invariants after concurrent operations
    let violations = registry.validate_invariants().await.unwrap();
    assert!(
        violations.is_empty(),
        "Invariants violated after concurrent operations: {:?}",
        violations
    );
}

#[tokio::test]
async fn test_registry_stats_accuracy() {
    let fixture = SecureChannelTestFixture::new().await;

    // Initial state
    let initial_stats = fixture.registry.get_registry_stats().await;
    assert_eq!(initial_stats.total_channels, 0);
    assert_eq!(initial_stats.active_context_count(), 0);
    assert_eq!(initial_stats.active_peer_count(), 0);

    // Create channel
    fixture.create_channel().await.unwrap();

    let after_create_stats = fixture.registry.get_registry_stats().await;
    assert_eq!(after_create_stats.total_channels, 1);
    assert_eq!(after_create_stats.establishing_channels, 1);
    assert_eq!(after_create_stats.active_context_count(), 1);
    assert_eq!(after_create_stats.active_peer_count(), 1);

    // Teardown
    fixture.registry.trigger_epoch_rotation(Epoch::new(2)).await;
    fixture.registry.process_teardown_queue().await.unwrap();

    let after_teardown_stats = fixture.registry.get_registry_stats().await;
    assert_eq!(after_teardown_stats.total_channels, 1);
    assert_eq!(after_teardown_stats.terminated_channels, 1);

    // Cleanup
    fixture.registry.cleanup_terminated_channels().await;

    let after_cleanup_stats = fixture.registry.get_registry_stats().await;
    assert_eq!(after_cleanup_stats.total_channels, 0);
    assert_eq!(after_cleanup_stats.active_context_count(), 0);
    assert_eq!(after_cleanup_stats.active_peer_count(), 0);
}