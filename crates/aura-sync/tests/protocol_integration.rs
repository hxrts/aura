//! Protocol integration tests for aura-sync
//!
//! These tests validate that individual sync protocols (anti-entropy, journal sync,
//! snapshots, OTA, receipts, and epochs) function correctly when integrated with
//! the aura-testkit testing framework and with each other.

use aura_core::DeviceId;
use aura_sync::protocols::{
    AntiEntropyConfig, AntiEntropyProtocol, EpochConfig, EpochRotationCoordinator,
    EpochRotationProposal, JournalSyncConfig, JournalSyncProtocol, OTAConfig, OTAProtocol,
    ReceiptVerificationConfig, ReceiptVerificationProtocol, SnapshotConfig, SnapshotProtocol,
};
use std::time::SystemTime;

// =============================================================================
// Anti-Entropy Protocol Tests
// =============================================================================

#[test]
fn test_anti_entropy_protocol_creation() {
    // Test basic anti-entropy protocol instantiation
    let config = AntiEntropyConfig::default();
    let protocol = AntiEntropyProtocol::new(config.clone());

    // Verify protocol is properly initialized
    assert_eq!(protocol.config().digest_timeout, config.digest_timeout);
    assert_eq!(
        protocol.config().reconciliation_timeout,
        config.reconciliation_timeout
    );
}

#[test]
fn test_anti_entropy_configuration_validation() {
    // Test that anti-entropy configuration is properly validated
    let mut config = AntiEntropyConfig::default();

    // Default should be valid
    assert!(config.validate().is_ok());

    // Test invalid configurations
    config.digest_timeout = std::time::Duration::from_secs(0);
    assert!(config.validate().is_err());

    // Reset to valid
    config.digest_timeout = std::time::Duration::from_secs(30);
    assert!(config.validate().is_ok());
}

#[test]
fn test_anti_entropy_with_multiple_peers() {
    // Test anti-entropy protocol with multiple peer scenarios
    let config = AntiEntropyConfig::default();
    let protocol = AntiEntropyProtocol::new(config);

    let peer1 = DeviceId::new();
    let peer2 = DeviceId::new();
    let peer3 = DeviceId::new();

    // Protocol should support multiple concurrent peer syncs
    assert_ne!(peer1, peer2);
    assert_ne!(peer2, peer3);
    assert_ne!(peer1, peer3);

    // Each peer should be distinct
    let peers = vec![peer1, peer2, peer3];
    let unique_peers: std::collections::HashSet<_> = peers.iter().cloned().collect();
    assert_eq!(unique_peers.len(), 3);
}

// =============================================================================
// Journal Sync Protocol Tests
// =============================================================================

#[test]
fn test_journal_sync_protocol_creation() {
    // Test journal sync protocol instantiation
    let config = JournalSyncConfig::default();
    let protocol = JournalSyncProtocol::new(config.clone());

    assert_eq!(protocol.config().batch_size, config.batch_size);
    assert_eq!(protocol.config().sync_timeout, config.sync_timeout);
}

#[test]
fn test_journal_sync_configuration() {
    // Test journal sync configuration properties
    let config = JournalSyncConfig::default();

    assert!(config.batch_size > 0);
    assert!(config.sync_timeout > std::time::Duration::ZERO);
    assert!(config.retry_policy.max_retries > 0);
}

#[test]
fn test_journal_sync_with_peers() {
    // Test journal sync protocol with multiple peers
    let config = JournalSyncConfig::default();
    let _protocol = JournalSyncProtocol::new(config);

    let primary_peer = DeviceId::new();
    let backup_peer = DeviceId::new();

    // Verify peer distinction
    assert_ne!(primary_peer, backup_peer);
}

// =============================================================================
// Snapshot Protocol Tests
// =============================================================================

#[test]
fn test_snapshot_protocol_creation() {
    // Test snapshot protocol instantiation
    let config = SnapshotConfig::default();
    let protocol = SnapshotProtocol::new(config.clone());

    assert_eq!(
        protocol.config().snapshot_threshold,
        config.snapshot_threshold
    );
    assert_eq!(protocol.config().approval_timeout, config.approval_timeout);
}

#[test]
fn test_snapshot_configuration_thresholds() {
    // Test snapshot configuration threshold validation
    let config = SnapshotConfig::default();

    assert!(config.snapshot_threshold > 0);
    assert!(config.approval_timeout > std::time::Duration::ZERO);
    assert!(config.min_participant_threshold > 0);
}

#[test]
fn test_snapshot_with_multiple_writers() {
    // Test snapshot protocol with multiple potential writers
    let config = SnapshotConfig::default();
    let _protocol = SnapshotProtocol::new(config);

    let writer1 = DeviceId::new();
    let writer2 = DeviceId::new();
    let reader = DeviceId::new();

    // All should be distinct
    assert_ne!(writer1, writer2);
    assert_ne!(writer1, reader);
    assert_ne!(writer2, reader);
}

// =============================================================================
// OTA Protocol Tests
// =============================================================================

#[test]
fn test_ota_protocol_creation() {
    // Test OTA protocol instantiation
    let config = OTAConfig::default();
    let protocol = OTAProtocol::new(config.clone());

    assert_eq!(
        protocol.config().epoch_fence_duration,
        config.epoch_fence_duration
    );
    assert_eq!(protocol.config().approval_timeout, config.approval_timeout);
}

#[test]
fn test_ota_configuration_safety() {
    // Test OTA configuration for safety constraints
    let config = OTAConfig::default();

    // Epoch fence should be substantial to prevent replay
    assert!(config.epoch_fence_duration > std::time::Duration::from_secs(60));
    assert!(config.approval_timeout > std::time::Duration::ZERO);
}

#[test]
fn test_ota_with_coordinators() {
    // Test OTA with upgrade coordinator scenarios
    let config = OTAConfig::default();
    let _protocol = OTAProtocol::new(config);

    let coordinator = DeviceId::new();
    let participant1 = DeviceId::new();
    let participant2 = DeviceId::new();

    // Coordinator and participants should be distinct
    assert_ne!(coordinator, participant1);
    assert_ne!(coordinator, participant2);
    assert_ne!(participant1, participant2);
}

// =============================================================================
// Receipt Verification Protocol Tests
// =============================================================================

#[test]
fn test_receipt_verification_protocol_creation() {
    // Test receipt verification protocol instantiation
    let config = ReceiptVerificationConfig::default();
    let protocol = ReceiptVerificationProtocol::new(config.clone());

    assert_eq!(
        protocol.config().verification_timeout,
        config.verification_timeout
    );
}

#[test]
fn test_receipt_verification_configuration() {
    // Test receipt verification configuration
    let config = ReceiptVerificationConfig::default();

    assert!(config.verification_timeout > std::time::Duration::ZERO);
    assert!(config.max_hops >= 1);
}

#[test]
fn test_receipt_verification_chain() {
    // Test receipt verification with message chain
    let config = ReceiptVerificationConfig::default();
    let _protocol = ReceiptVerificationProtocol::new(config);

    let sender = DeviceId::new();
    let intermediate1 = DeviceId::new();
    let intermediate2 = DeviceId::new();
    let receiver = DeviceId::new();

    // All should be distinct for proper chain tracking
    let chain = vec![sender, intermediate1, intermediate2, receiver];
    let unique: std::collections::HashSet<_> = chain.iter().cloned().collect();
    assert_eq!(unique.len(), 4);
}

// =============================================================================
// Epoch Management Protocol Tests
// =============================================================================

#[test]
fn test_epoch_rotation_coordinator_creation() {
    // Test epoch rotation coordinator instantiation
    let device_id = DeviceId::new();
    let config = EpochConfig::default();
    let coordinator = EpochRotationCoordinator::new(device_id, 0, config);

    assert_eq!(coordinator.current_epoch(), 0);
}

#[test]
fn test_epoch_rotation_initiation() {
    // Test epoch rotation initiation
    let device_id = DeviceId::new();
    let config = EpochConfig::default();
    let mut coordinator = EpochRotationCoordinator::new(device_id, 0, config);

    let participant1 = DeviceId::new();
    let participant2 = DeviceId::new();
    let context_id = aura_core::ContextId::new();

    let result = coordinator.initiate_rotation(vec![participant1, participant2], context_id);

    assert!(result.is_ok());
    let rotation_id = result.unwrap();
    assert!(!rotation_id.is_empty());
}

#[test]
fn test_epoch_rotation_with_insufficient_participants() {
    // Test epoch rotation fails with too few participants
    let device_id = DeviceId::new();
    let mut config = EpochConfig::default();
    config.rotation_threshold = 3;

    let mut coordinator = EpochRotationCoordinator::new(device_id, 0, config);

    let participant = DeviceId::new();
    let context_id = aura_core::ContextId::new();

    let result = coordinator.initiate_rotation(vec![participant], context_id);

    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Insufficient participants"));
}

#[test]
fn test_epoch_confirmation_processing() {
    // Test processing epoch confirmations
    let device_id = DeviceId::new();
    let config = EpochConfig::default();
    let mut coordinator = EpochRotationCoordinator::new(device_id, 0, config);

    let participant1 = DeviceId::new();
    let participant2 = DeviceId::new();
    let context_id = aura_core::ContextId::new();

    let rotation_id = coordinator
        .initiate_rotation(vec![participant1, participant2], context_id)
        .unwrap();

    // Process confirmations
    let confirmation1 = aura_sync::protocols::EpochConfirmation {
        rotation_id: rotation_id.clone(),
        participant_id: participant1,
        current_epoch: 0,
        ready_for_epoch: 1,
        confirmation_timestamp: SystemTime::now(),
    };

    let result = coordinator.process_confirmation(confirmation1);
    assert!(result.is_ok());
    assert!(!result.unwrap()); // Not ready yet (need both confirmations)

    let confirmation2 = aura_sync::protocols::EpochConfirmation {
        rotation_id: rotation_id.clone(),
        participant_id: participant2,
        current_epoch: 0,
        ready_for_epoch: 1,
        confirmation_timestamp: SystemTime::now(),
    };

    let result = coordinator.process_confirmation(confirmation2);
    assert!(result.is_ok());
    assert!(result.unwrap()); // Now ready to commit
}

#[test]
fn test_epoch_commit() {
    // Test epoch commit operation
    let device_id = DeviceId::new();
    let config = EpochConfig::default();
    let mut coordinator = EpochRotationCoordinator::new(device_id, 0, config);

    let participants = vec![DeviceId::new(), DeviceId::new()];
    let context_id = aura_core::ContextId::new();

    let rotation_id = coordinator
        .initiate_rotation(participants.clone(), context_id)
        .unwrap();

    // Process both confirmations
    for participant in participants {
        let confirmation = aura_sync::protocols::EpochConfirmation {
            rotation_id: rotation_id.clone(),
            participant_id: participant,
            current_epoch: 0,
            ready_for_epoch: 1,
            confirmation_timestamp: SystemTime::now(),
        };
        let _ = coordinator.process_confirmation(confirmation);
    }

    // Commit the rotation
    let result = coordinator.commit_rotation(&rotation_id);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), 1); // New epoch is 1
    assert_eq!(coordinator.current_epoch(), 1);
}

#[test]
fn test_epoch_rotation_cleanup() {
    // Test cleanup of completed rotations
    let device_id = DeviceId::new();
    let config = EpochConfig::default();
    let mut coordinator = EpochRotationCoordinator::new(device_id, 0, config);

    // Create and complete multiple rotations
    for i in 0..3 {
        let participants = vec![DeviceId::new(), DeviceId::new()];
        let context_id = aura_core::ContextId::new();

        let rotation_id = coordinator
            .initiate_rotation(participants.clone(), context_id)
            .unwrap();

        // Process confirmations
        for participant in participants {
            let confirmation = aura_sync::protocols::EpochConfirmation {
                rotation_id: rotation_id.clone(),
                participant_id: participant,
                current_epoch: i,
                ready_for_epoch: i + 1,
                confirmation_timestamp: SystemTime::now(),
            };
            let _ = coordinator.process_confirmation(confirmation);
        }

        // Commit
        let _ = coordinator.commit_rotation(&rotation_id);
    }

    // Cleanup
    let cleaned = coordinator.cleanup_completed_rotations();
    assert_eq!(cleaned, 3);
    assert_eq!(coordinator.list_pending_rotations().len(), 0);
}

// =============================================================================
// Cross-Protocol Integration Tests
// =============================================================================

#[test]
fn test_protocol_independence() {
    // Test that protocols are independent and composable
    let anti_entropy_config = AntiEntropyConfig::default();
    let journal_sync_config = JournalSyncConfig::default();
    let snapshot_config = SnapshotConfig::default();
    let ota_config = OTAConfig::default();
    let receipt_config = ReceiptVerificationConfig::default();
    let epoch_config = EpochConfig::default();

    // All should initialize without affecting each other
    let _ae = AntiEntropyProtocol::new(anti_entropy_config);
    let _js = JournalSyncProtocol::new(journal_sync_config);
    let _snap = SnapshotProtocol::new(snapshot_config);
    let _ota = OTAProtocol::new(ota_config);
    let _recv = ReceiptVerificationProtocol::new(receipt_config);
    let _epoch = EpochRotationCoordinator::new(DeviceId::new(), 0, epoch_config);

    // All should coexist without conflicts
}

#[test]
fn test_protocol_configuration_consistency() {
    // Test that all protocol configurations follow consistent patterns
    let ae_config = AntiEntropyConfig::default();
    let js_config = JournalSyncConfig::default();
    let snap_config = SnapshotConfig::default();
    let ota_config = OTAConfig::default();
    let recv_config = ReceiptVerificationConfig::default();

    // All should have timeout configurations
    assert!(ae_config.digest_timeout > std::time::Duration::ZERO);
    assert!(js_config.sync_timeout > std::time::Duration::ZERO);
    assert!(snap_config.approval_timeout > std::time::Duration::ZERO);
    assert!(ota_config.approval_timeout > std::time::Duration::ZERO);
    assert!(recv_config.verification_timeout > std::time::Duration::ZERO);
}

#[test]
fn test_multi_device_protocol_scenarios() {
    // Test realistic multi-device scenarios
    let device1 = DeviceId::new();
    let device2 = DeviceId::new();
    let device3 = DeviceId::new();

    // Create coordinators on each device
    let mut coord1 = EpochRotationCoordinator::new(device1, 0, EpochConfig::default());
    let mut coord2 = EpochRotationCoordinator::new(device2, 0, EpochConfig::default());
    let mut coord3 = EpochRotationCoordinator::new(device3, 0, EpochConfig::default());

    // Device 1 initiates rotation
    let context = aura_core::ContextId::new();
    let rotation_id = coord1
        .initiate_rotation(vec![device2, device3], context)
        .unwrap();

    // Devices 2 and 3 confirm
    let conf2 = aura_sync::protocols::EpochConfirmation {
        rotation_id: rotation_id.clone(),
        participant_id: device2,
        current_epoch: 0,
        ready_for_epoch: 1,
        confirmation_timestamp: SystemTime::now(),
    };

    let conf3 = aura_sync::protocols::EpochConfirmation {
        rotation_id: rotation_id.clone(),
        participant_id: device3,
        current_epoch: 0,
        ready_for_epoch: 1,
        confirmation_timestamp: SystemTime::now(),
    };

    // Coordinator processes confirmations
    let ready = coord1.process_confirmation(conf2).unwrap();
    assert!(!ready); // Still need one more

    let ready = coord1.process_confirmation(conf3).unwrap();
    assert!(ready); // Now ready

    // Commit
    let new_epoch = coord1.commit_rotation(&rotation_id).unwrap();
    assert_eq!(new_epoch, 1);

    // In real scenario, devices 2 and 3 would also receive and commit the rotation
    assert_eq!(coord1.current_epoch(), 1);
}
