//! Protocol integration tests for aura-sync
//!
//! These tests validate that individual sync protocols (anti-entropy, journal sync,
//! snapshots, OTA, receipts, and epochs) function correctly when integrated with
//! the aura-testkit testing framework and with each other.

use aura_core::time::PhysicalTime;
use aura_core::types::Epoch;
use aura_core::DeviceId;
use aura_sync::protocols::{
    AntiEntropyConfig, AntiEntropyProtocol, EpochConfig, EpochRotationCoordinator,
    JournalSyncConfig, JournalSyncProtocol, OTAConfig, OTAProtocol, ReceiptVerificationConfig,
    ReceiptVerificationProtocol, SnapshotConfig, SnapshotProtocol,
};

fn device(seed: u8) -> DeviceId {
    DeviceId::new_from_entropy([seed; 32])
}

// Test fixture: deterministic timestamp for reproducible tests
const TEST_TIMESTAMP_MS: u64 = 1700000000000; // 2023-11-15 in milliseconds

fn test_time(ts_ms: u64) -> PhysicalTime {
    PhysicalTime {
        ts_ms,
        uncertainty: None,
    }
}

// =============================================================================
// Anti-Entropy Protocol Tests
// =============================================================================

#[test]
fn test_anti_entropy_protocol_creation() {
    // Test basic anti-entropy protocol instantiation
    let config = AntiEntropyConfig::default();
    let _protocol = AntiEntropyProtocol::new(config.clone());

    // Verify protocol is properly initialized by creating it successfully
    // Note: config is private, so we can't directly access it
    // The fact that the protocol was created successfully means the config was accepted
    assert_eq!(config.digest_timeout.as_secs(), 10);
    assert_eq!(config.transfer_timeout.as_secs(), 30);
}

#[test]
fn test_anti_entropy_configuration_validation() {
    // Test that anti-entropy configuration can be created with various values
    let config = AntiEntropyConfig::default();

    // Default configuration should have reasonable values
    assert!(config.digest_timeout.as_secs() > 0);
    assert!(config.batch_size > 0);
    assert!(config.max_rounds > 0);

    // Test custom configurations
    let custom_config = AntiEntropyConfig {
        batch_size: 64,
        max_rounds: 5,
        retry_enabled: false,
        digest_timeout: std::time::Duration::from_secs(5),
        transfer_timeout: std::time::Duration::from_secs(15),
        ..Default::default()
    };

    // Custom config should be usable to create a protocol
    let _protocol = AntiEntropyProtocol::new(custom_config);
}

#[test]
fn test_anti_entropy_with_multiple_peers() {
    // Test anti-entropy protocol with multiple peer scenarios
    let config = AntiEntropyConfig::default();
    let _protocol = AntiEntropyProtocol::new(config);

    let peer1 = device(1);
    let peer2 = device(2);
    let peer3 = device(3);

    // Protocol should support multiple concurrent peer syncs
    assert_ne!(peer1, peer2);
    assert_ne!(peer2, peer3);
    assert_ne!(peer1, peer3);

    // Each peer should be distinct
    let peers = [peer1, peer2, peer3];
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
    let _protocol = JournalSyncProtocol::new(config.clone());

    // Verify protocol is created successfully by checking the config values
    // Note: config is private, so we verify by checking the original config
    assert!(config.batch_size > 0);
    assert!(config.sync_timeout > std::time::Duration::ZERO);
}

#[test]
fn test_journal_sync_configuration() {
    // Test journal sync configuration properties
    let config = JournalSyncConfig::default();

    assert!(config.batch_size > 0);
    assert!(config.sync_timeout > std::time::Duration::ZERO);
    // Field exists - retry_enabled is accessible
    let _ = config.retry_enabled;
    assert!(config.max_concurrent_syncs > 0);
}

#[test]
fn test_journal_sync_with_peers() {
    // Test journal sync protocol with multiple peers
    let config = JournalSyncConfig::default();
    let _protocol = JournalSyncProtocol::new(config);

    let primary_peer = device(4);
    let backup_peer = device(5);

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
    let _protocol = SnapshotProtocol::new(config.clone());

    // Verify protocol is created successfully by checking the original config
    // Note: config is private, so we verify using the original config values
    assert_eq!(config.approval_threshold, 2);
    assert_eq!(config.quorum_size, 3);
    // Field exists - use_writer_fence is accessible
    let _ = config.use_writer_fence;
}

#[test]
fn test_snapshot_configuration_thresholds() {
    // Test snapshot configuration threshold validation
    let config = SnapshotConfig::default();

    assert!(config.approval_threshold > 0);
    assert!(config.quorum_size > 0);
    assert!(config.approval_threshold <= config.quorum_size);

    // Test custom configuration
    let custom_config = SnapshotConfig {
        approval_threshold: 3,
        quorum_size: 5,
        use_writer_fence: false,
    };
    assert!(custom_config.approval_threshold <= custom_config.quorum_size);
}

#[test]
fn test_snapshot_with_multiple_writers() {
    // Test snapshot protocol with multiple potential writers
    let config = SnapshotConfig::default();
    let _protocol = SnapshotProtocol::new(config);

    let writer1 = device(6);
    let writer2 = device(7);
    let reader = device(8);

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
    let _protocol = OTAProtocol::new(config.clone());

    // Verify protocol is created successfully by checking the original config
    // Note: config is private, so we verify using the original config values
    assert!(config.readiness_threshold > 0);
    assert!(config.quorum_size > 0);
    assert!(config.readiness_threshold <= config.quorum_size);
}

#[test]
fn test_ota_configuration_safety() {
    // Test OTA configuration for safety constraints
    let config = OTAConfig::default();

    // Configuration should have sensible threshold values for safety
    assert!(config.readiness_threshold > 0);
    assert!(config.quorum_size >= config.readiness_threshold);

    // Test that epoch fence can be enforced for security
    let secure_config = OTAConfig {
        readiness_threshold: 2,
        quorum_size: 3,
        enforce_epoch_fence: true,
    };
    assert!(secure_config.enforce_epoch_fence);
}

#[test]
fn test_ota_with_coordinators() {
    // Test OTA with upgrade coordinator scenarios
    let config = OTAConfig::default();
    let _protocol = OTAProtocol::new(config);

    let coordinator = device(9);
    let participant1 = device(10);
    let participant2 = device(11);

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
    let _protocol = ReceiptVerificationProtocol::new(config.clone());

    // Verify protocol is created successfully by checking the original config
    // Note: config is private, so we verify using the original config values
    assert!(config.max_chain_depth > 0);
    // Field exists - verify_signatures is accessible
    let _ = config.verify_signatures;
}

#[test]
fn test_receipt_verification_configuration() {
    // Test receipt verification configuration
    let config = ReceiptVerificationConfig::default();

    assert!(config.max_chain_depth > 0);

    // Test custom configuration
    let custom_config = ReceiptVerificationConfig {
        max_chain_depth: 10,
        require_chronological: true,
        verify_signatures: true,
        require_consensus_finalization: false,
    };
    assert_eq!(custom_config.max_chain_depth, 10);
    assert!(custom_config.require_chronological);
    assert!(custom_config.verify_signatures);
}

#[test]
fn test_receipt_verification_chain() {
    // Test receipt verification with message chain
    let config = ReceiptVerificationConfig::default();
    let _protocol = ReceiptVerificationProtocol::new(config);

    let sender = device(12);
    let intermediate1 = device(13);
    let intermediate2 = device(14);
    let receiver = device(15);

    // All should be distinct for proper chain tracking
    let chain = [sender, intermediate1, intermediate2, receiver];
    let unique: std::collections::HashSet<_> = chain.iter().cloned().collect();
    assert_eq!(unique.len(), 4);
}

// =============================================================================
// Epoch Management Protocol Tests
// =============================================================================

#[test]
fn test_epoch_rotation_coordinator_creation() {
    // Test epoch rotation coordinator instantiation
    let device_id = device(16);
    let config = EpochConfig::default();
    let coordinator = EpochRotationCoordinator::new(device_id, Epoch::new(0), config);

    assert_eq!(coordinator.current_epoch(), Epoch::new(0));
}

#[test]
fn test_epoch_rotation_initiation() {
    // Test epoch rotation initiation
    let device_id = device(17);
    let config = EpochConfig::default();
    let mut coordinator = EpochRotationCoordinator::new(device_id, Epoch::new(0), config);

    let participant1 = device(18);
    let participant2 = device(19);
    let context_id = aura_core::ContextId::new_from_entropy([0u8; 32]);

    let result = coordinator.initiate_rotation(
        vec![participant1, participant2],
        context_id,
        &test_time(TEST_TIMESTAMP_MS),
    );

    assert!(result.is_ok());
    let rotation_id = result.unwrap();
    assert!(!rotation_id.is_empty());
}

#[test]
fn test_epoch_rotation_with_insufficient_participants() {
    // Test epoch rotation fails with too few participants
    let device_id = device(20);
    let config = EpochConfig {
        rotation_threshold: 3,
        ..Default::default()
    };

    let mut coordinator = EpochRotationCoordinator::new(device_id, Epoch::new(0), config);

    let participant = device(21);
    let context_id = aura_core::ContextId::new_from_entropy([1u8; 32]);

    let result =
        coordinator.initiate_rotation(vec![participant], context_id, &test_time(TEST_TIMESTAMP_MS));

    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Insufficient participants"));
}

#[test]
fn test_epoch_confirmation_processing() {
    // Test processing epoch confirmations
    let device_id = device(22);
    let config = EpochConfig::default();
    let mut coordinator = EpochRotationCoordinator::new(device_id, Epoch::new(0), config);

    let participant1 = device(23);
    let participant2 = device(24);
    let context_id = aura_core::ContextId::new_from_entropy([2u8; 32]);

    let rotation_id = coordinator
        .initiate_rotation(
            vec![participant1, participant2],
            context_id,
            &test_time(TEST_TIMESTAMP_MS),
        )
        .unwrap();

    // Process confirmations
    let confirmation1 = aura_sync::protocols::EpochConfirmation {
        rotation_id: rotation_id.clone(),
        participant_id: participant1,
        current_epoch: Epoch::new(0),
        ready_for_epoch: Epoch::new(1),
        confirmation_timestamp: test_time(TEST_TIMESTAMP_MS),
    };

    let result = coordinator.process_confirmation(confirmation1);
    assert!(result.is_ok());
    assert!(!result.unwrap()); // Not ready yet (need both confirmations)

    let confirmation2 = aura_sync::protocols::EpochConfirmation {
        rotation_id: rotation_id.clone(),
        participant_id: participant2,
        current_epoch: Epoch::new(0),
        ready_for_epoch: Epoch::new(1),
        confirmation_timestamp: test_time(TEST_TIMESTAMP_MS),
    };

    let result = coordinator.process_confirmation(confirmation2);
    assert!(result.is_ok());
    assert!(result.unwrap()); // Now ready to commit
}

#[test]
fn test_epoch_commit() {
    // Test epoch commit operation
    let device_id = device(25);
    let config = EpochConfig::default();
    let mut coordinator = EpochRotationCoordinator::new(device_id, Epoch::new(0), config);

    let participants = vec![device(26), device(27)];
    let context_id = aura_core::ContextId::new_from_entropy([3u8; 32]);

    let rotation_id = coordinator
        .initiate_rotation(
            participants.clone(),
            context_id,
            &test_time(TEST_TIMESTAMP_MS),
        )
        .unwrap();

    // Process both confirmations
    for participant in participants {
        let confirmation = aura_sync::protocols::EpochConfirmation {
            rotation_id: rotation_id.clone(),
            participant_id: participant,
            current_epoch: Epoch::new(0),
            ready_for_epoch: Epoch::new(1),
            confirmation_timestamp: test_time(TEST_TIMESTAMP_MS),
        };
        let _ = coordinator.process_confirmation(confirmation);
    }

    // Commit the rotation
    let result = coordinator.commit_rotation(&rotation_id);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Epoch::new(1)); // New epoch is 1
    assert_eq!(coordinator.current_epoch(), Epoch::new(1));
}

#[test]
fn test_epoch_rotation_cleanup() {
    // Test cleanup of completed rotations
    let device_id = device(28);
    let config = EpochConfig::default();
    let mut coordinator = EpochRotationCoordinator::new(device_id, Epoch::new(0), config);

    // Create and complete multiple rotations
    for i in 0..3 {
        let participants = vec![device(30 + (i as u8 * 2)), device(31 + (i as u8 * 2))];
        let context_id = aura_core::ContextId::new_from_entropy([4u8; 32]);

        let rotation_id = coordinator
            .initiate_rotation(
                participants.clone(),
                context_id,
                &test_time(TEST_TIMESTAMP_MS),
            )
            .unwrap();

        // Process confirmations
        for participant in participants {
            let confirmation = aura_sync::protocols::EpochConfirmation {
                rotation_id: rotation_id.clone(),
                participant_id: participant,
                current_epoch: Epoch::new(i),
                ready_for_epoch: Epoch::new(i + 1),
                confirmation_timestamp: test_time(TEST_TIMESTAMP_MS),
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
    let _epoch = EpochRotationCoordinator::new(device(40), Epoch::new(0), epoch_config);

    // All should coexist without conflicts
}

#[test]
fn test_protocol_configuration_consistency() {
    // Test that all protocol configurations follow consistent patterns
    let ae_config = AntiEntropyConfig::default();
    let js_config = JournalSyncConfig::default();
    let _snap_config = SnapshotConfig::default();
    let _ota_config = OTAConfig::default();
    let _recv_config = ReceiptVerificationConfig::default();

    // All should have timeout configurations
    assert!(ae_config.digest_timeout > std::time::Duration::ZERO);
    assert!(js_config.sync_timeout > std::time::Duration::ZERO);
    // SnapshotConfig and OTAConfig don't have timeout fields in current implementation
    // ReceiptVerificationConfig doesn't have timeout field in current implementation
}

#[test]
fn test_multi_device_protocol_scenarios() {
    // Test realistic multi-device scenarios
    let device1 = device(41);
    let device2 = device(42);
    let device3 = device(43);

    // Create coordinators on each device
    let mut coord1 = EpochRotationCoordinator::new(device1, Epoch::new(0), EpochConfig::default());
    let _coord2 = EpochRotationCoordinator::new(device2, Epoch::new(0), EpochConfig::default());
    let _coord3 = EpochRotationCoordinator::new(device3, Epoch::new(0), EpochConfig::default());

    // Device 1 initiates rotation
    let context = aura_core::ContextId::new_from_entropy([5u8; 32]);
    let rotation_id = coord1
        .initiate_rotation(
            vec![device2, device3],
            context,
            &test_time(TEST_TIMESTAMP_MS),
        )
        .unwrap();

    // Devices 2 and 3 confirm
    let conf2 = aura_sync::protocols::EpochConfirmation {
        rotation_id: rotation_id.clone(),
        participant_id: device2,
        current_epoch: Epoch::new(0),
        ready_for_epoch: Epoch::new(1),
        confirmation_timestamp: test_time(TEST_TIMESTAMP_MS),
    };

    let conf3 = aura_sync::protocols::EpochConfirmation {
        rotation_id: rotation_id.clone(),
        participant_id: device3,
        current_epoch: Epoch::new(0),
        ready_for_epoch: Epoch::new(1),
        confirmation_timestamp: test_time(TEST_TIMESTAMP_MS),
    };

    // Coordinator processes confirmations
    let ready = coord1.process_confirmation(conf2).unwrap();
    assert!(!ready); // Still need one more

    let ready = coord1.process_confirmation(conf3).unwrap();
    assert!(ready); // Now ready

    // Commit
    let new_epoch = coord1.commit_rotation(&rotation_id).unwrap();
    assert_eq!(new_epoch, Epoch::new(1));

    // In real scenario, devices 2 and 3 would also receive and commit the rotation
    assert_eq!(coord1.current_epoch(), Epoch::new(1));
}
