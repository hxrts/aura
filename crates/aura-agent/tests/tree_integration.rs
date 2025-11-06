//! Integration tests for TreeCoordinator
//!
//! These tests verify end-to-end functionality of tree operations through the
//! TreeCoordinator API, testing the integration between agent, journal handler,
//! and tree state management.

use aura_agent::{TreeCoordinator, TreeError};
use aura_crypto::Effects;
use aura_protocol::handlers::AuraHandlerFactory;
use aura_types::{
    identifiers::DeviceId,
    ledger::{Intent, IntentId, Priority, ThresholdSignature},
    tree::{AffectedPath, Commitment, LeafId, LeafIndex, LeafNode, LeafRole, NodeIndex, Policy, TreeOperation},
};
use aura_journal::tree::node::{KeyPackage, LeafMetadata};
use std::collections::BTreeMap;

/// Test basic coordinator creation and initialization
#[tokio::test]
async fn test_coordinator_initialization() {
    let effects = Effects::for_test("coordinator_init_test");
    let device_id = DeviceId(effects.gen_uuid());
    let handler = AuraHandlerFactory::for_testing(device_id).unwrap();
    let coordinator = TreeCoordinator::new(handler, device_id);

    // Should be able to fetch empty tree
    let tree = coordinator.get_current_tree().await.unwrap();
    assert_eq!(tree.leaf_count(), 0, "Tree should be empty initially");

    // Should be able to get stats
    let stats = coordinator.get_stats().await.unwrap();
    assert_eq!(stats.num_ops, 0, "Should have no ops initially");
    assert_eq!(stats.num_intents, 0, "Should have no intents initially");
}

/// Test intent submission without waiting for completion
#[tokio::test]
async fn test_intent_submission() {
    let effects = Effects::for_test("coordinator_init_test");
    let device_id = DeviceId(effects.gen_uuid());
    let handler = AuraHandlerFactory::for_testing(device_id).unwrap();
    let coordinator = TreeCoordinator::new(handler, device_id);

    // Create a test intent
    let leaf_node = LeafNode {
        leaf_id: LeafId::new(),
        leaf_index: LeafIndex(0),
        role: LeafRole::Device,
        public_key: KeyPackage {
            signing_key: vec![0u8; 32],
            encryption_key: None,
        },
        metadata: LeafMetadata::default(),
    };
    
    let intent = Intent {
        intent_id: IntentId::new(),
        op: TreeOperation::AddLeaf {
            leaf_node,
            affected_path: AffectedPath::new(),
        },
        path_span: vec![NodeIndex::new(0)],
        snapshot_commitment: Commitment::new([0u8; 32]),
        priority: Priority::from(100),
        author: device_id,
        created_at: effects.now().unwrap_or(0),
        metadata: std::collections::BTreeMap::new(),
    };

    // Submit intent
    let intent_id = coordinator.submit_intent(intent).await.unwrap();
    assert!(!intent_id.0.is_empty(), "Intent ID should not be empty");

    // Verify intent appears in pending list
    let pending = coordinator.list_pending_intents().await.unwrap();
    assert_eq!(pending.len(), 1, "Should have one pending intent");
    assert_eq!(pending[0].intent_id, intent_id, "Intent ID should match");
}

/// Test device membership queries
#[tokio::test]
async fn test_device_membership_queries() {
    let effects = Effects::for_test("coordinator_init_test");
    let device_id = DeviceId(effects.gen_uuid());
    let handler = AuraHandlerFactory::for_testing(device_id).unwrap();
    let coordinator = TreeCoordinator::new(handler, device_id);

    // Initially no devices
    let devices = coordinator.list_devices().await.unwrap();
    assert_eq!(devices.len(), 0, "Should have no devices initially");

    // Check device is not a member
    let is_member = coordinator.is_device_member(device_id).await.unwrap();
    assert!(!is_member, "Device should not be a member initially");
}

/// Test guardian listing
#[tokio::test]
async fn test_guardian_listing() {
    let effects = Effects::for_test("coordinator_init_test");
    let device_id = DeviceId(effects.gen_uuid());
    let handler = AuraHandlerFactory::for_testing(device_id).unwrap();
    let coordinator = TreeCoordinator::new(handler, device_id);

    // Initially no guardians
    let guardians = coordinator.list_guardians().await.unwrap();
    assert_eq!(guardians.len(), 0, "Should have no guardians initially");
}

/// Test epoch and commitment queries
#[tokio::test]
async fn test_epoch_and_commitment_queries() {
    let effects = Effects::for_test("coordinator_init_test");
    let device_id = DeviceId(effects.gen_uuid());
    let handler = AuraHandlerFactory::for_testing(device_id).unwrap();
    let coordinator = TreeCoordinator::new(handler, device_id);

    // Get latest epoch (should be None for empty journal)
    let epoch = coordinator.get_latest_epoch().await.unwrap();
    assert!(epoch.is_none(), "Should have no epoch initially");

    // Get current commitment
    let commitment = coordinator.get_current_commitment().await.unwrap();
    assert_eq!(commitment.0.len(), 32, "Commitment should be 32 bytes");
}

/// Test tree cache behavior
#[tokio::test]
async fn test_tree_cache_behavior() {
    let effects = Effects::for_test("coordinator_init_test");
    let device_id = DeviceId(effects.gen_uuid());
    let handler = AuraHandlerFactory::for_testing(device_id).unwrap();
    let coordinator = TreeCoordinator::new(handler, device_id);

    // First fetch should populate cache
    let tree1 = coordinator.get_current_tree().await.unwrap();
    assert_eq!(tree1.leaf_count(), 0);

    // Second fetch should use cache (same result)
    let tree2 = coordinator.get_current_tree().await.unwrap();
    assert_eq!(tree2.leaf_count(), tree1.leaf_count());

    // Invalidate cache
    coordinator.invalidate_cache().await;

    // Third fetch should repopulate cache
    let tree3 = coordinator.get_current_tree().await.unwrap();
    assert_eq!(tree3.leaf_count(), tree1.leaf_count());
}

/// Test coordinator with custom timeout
#[tokio::test]
async fn test_custom_timeout() {
    use std::time::Duration;

    let effects = Effects::for_test("coordinator_init_test");
    let device_id = DeviceId(effects.gen_uuid());
    let handler = AuraHandlerFactory::for_testing(device_id).unwrap();
    let coordinator = TreeCoordinator::with_timeout(handler, device_id, Duration::from_secs(5));

    // Should work normally with custom timeout
    let tree = coordinator.get_current_tree().await.unwrap();
    assert_eq!(tree.leaf_count(), 0);
}

/// Test stats reporting
#[tokio::test]
async fn test_stats_reporting() {
    let effects = Effects::for_test("coordinator_init_test");
    let device_id = DeviceId(effects.gen_uuid());
    let handler = AuraHandlerFactory::for_testing(device_id).unwrap();
    let coordinator = TreeCoordinator::new(handler, device_id);

    let stats = coordinator.get_stats().await.unwrap();
    assert_eq!(stats.num_ops, 0, "Should have 0 ops");
    assert_eq!(stats.num_intents, 0, "Should have 0 intents");
    assert_eq!(stats.num_tombstones, 0, "Should have 0 tombstones");
    assert!(stats.latest_epoch.is_none(), "Should have no epoch");
    assert_eq!(stats.num_devices, 0, "Should have 0 devices");
    assert_eq!(stats.num_guardians, 0, "Should have 0 guardians");
}

// The following tests are marked as #[ignore] because they require full
// TreeSession choreography infrastructure which needs integration with
// the choreography layer. These will be enabled once Phase 4.3 infrastructure
// is complete.

/// Test full device onboarding flow
#[tokio::test]
#[ignore = "Requires TreeSession choreography infrastructure"]
async fn test_full_device_onboarding() {
    let effects = Effects::for_test("coordinator_init_test");
    let device_id = DeviceId(effects.gen_uuid());
    let handler = AuraHandlerFactory::for_testing(device_id).unwrap();
    let coordinator = TreeCoordinator::new(handler, device_id);

    // Add a new device
    let new_device = DeviceId(effects.gen_uuid());
    let public_key = vec![1u8; 32];

    let leaf_index = coordinator
        .add_device(new_device, public_key)
        .await
        .unwrap();
    assert_eq!(leaf_index, LeafIndex(0), "Should get first leaf index");

    // Verify device is now a member
    let is_member = coordinator.is_device_member(new_device).await.unwrap();
    assert!(is_member, "Device should be a member after adding");

    // Verify it appears in device list
    let devices = coordinator.list_devices().await.unwrap();
    assert_eq!(devices.len(), 1, "Should have one device");
    assert!(devices.contains(&new_device), "Should contain new device");

    // Verify tree state updated
    let tree = coordinator.get_current_tree().await.unwrap();
    assert_eq!(tree.leaf_count(), 1, "Tree should have one leaf");
}

/// Test device removal flow
#[tokio::test]
#[ignore = "Requires TreeSession choreography infrastructure"]
async fn test_device_removal() {
    let effects = Effects::for_test("coordinator_init_test");
    let device_id = DeviceId(effects.gen_uuid());
    let handler = AuraHandlerFactory::for_testing(device_id).unwrap();
    let coordinator = TreeCoordinator::new(handler, device_id);

    // First add a device
    let target_device = DeviceId(effects.gen_uuid());
    let public_key = vec![1u8; 32];
    coordinator
        .add_device(target_device, public_key)
        .await
        .unwrap();

    // Then remove it
    coordinator.remove_device(target_device).await.unwrap();

    // Verify device is no longer a member
    let is_member = coordinator.is_device_member(target_device).await.unwrap();
    assert!(!is_member, "Device should not be a member after removal");

    // Verify device list is empty
    let devices = coordinator.list_devices().await.unwrap();
    assert_eq!(devices.len(), 0, "Should have no devices after removal");
}

/// Test key rotation flow
#[tokio::test]
#[ignore = "Requires TreeSession choreography infrastructure"]
async fn test_key_rotation() {
    let effects = Effects::for_test("coordinator_init_test");
    let device_id = DeviceId(effects.gen_uuid());
    let handler = AuraHandlerFactory::for_testing(device_id).unwrap();
    let coordinator = TreeCoordinator::new(handler, device_id);

    // Add a device first
    let target_device = DeviceId(effects.gen_uuid());
    let public_key = vec![1u8; 32];
    coordinator
        .add_device(target_device, public_key)
        .await
        .unwrap();

    // Get initial epoch
    let initial_epoch = coordinator.get_latest_epoch().await.unwrap();

    // Rotate the device keys
    coordinator.rotate_device(target_device).await.unwrap();

    // Verify epoch incremented (forward secrecy)
    let new_epoch = coordinator.get_latest_epoch().await.unwrap();
    assert!(
        new_epoch > initial_epoch,
        "Epoch should increment after rotation"
    );
}

/// Test recovery ceremony flow
#[tokio::test]
#[ignore = "Requires TreeSession choreography infrastructure"]
async fn test_recovery_ceremony() {
    let effects = Effects::for_test("coordinator_init_test");
    let device_id = DeviceId(effects.gen_uuid());
    let handler = AuraHandlerFactory::for_testing(device_id).unwrap();
    let coordinator = TreeCoordinator::new(handler, device_id);

    // Start recovery
    let capability = coordinator.start_recovery().await.unwrap();

    // Verify capability properties
    assert_eq!(
        capability.guardian_threshold.0, 2,
        "Should require 2 guardians"
    );
    assert_eq!(capability.guardian_threshold.1, 3, "Should be 2-of-3");
    assert!(
        capability.expires_at > current_timestamp(),
        "Should not be expired"
    );
    assert_eq!(capability.requester, device_id, "Requester should match");

    // Verify capability can be validated
    // (Full validation requires capability as CapabilityRef)
}

/// Test capability validation
#[tokio::test]
#[ignore = "Requires capability signature infrastructure"]
async fn test_capability_validation() {
    let effects = Effects::for_test("coordinator_init_test");
    let device_id = DeviceId(effects.gen_uuid());
    let handler = AuraHandlerFactory::for_testing(device_id).unwrap();
    let coordinator = TreeCoordinator::new(handler, device_id);

    // Create a test capability (would normally come from recovery ceremony)
    // let capability = CapabilityRef { ... };

    // Validate it
    // let is_valid = coordinator.validate_capability(&capability).await.unwrap();
    // assert!(is_valid, "Fresh capability should be valid");

    // TODO: Test expiration checking
    // TODO: Test revocation checking
}

/// Helper to get current timestamp using effects
fn current_timestamp() -> u64 {
    let effects = Effects::for_test("current_timestamp_test");
    effects.now().unwrap_or(0)
}
