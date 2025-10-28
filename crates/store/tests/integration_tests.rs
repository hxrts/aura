//! Deterministic Integration Tests for SSB + Storage
//!
//! Uses the Aura simulator infrastructure for fully deterministic testing
//! with controlled time, network, and randomness.
//!
//! Reference: work/ssb_storage.md Phase 5.2

use aura_crypto::Effects;
use aura_journal::serialization::Serializable;
use aura_store::{
    manifest::{Permission, ResourceScope, SignatureShare, StorageOperation, ThresholdSignature},
    social_storage::{
        SocialStoragePeerDiscovery, StorageCapabilityAnnouncement, StorageMetrics, StoragePeer,
        StorageRequirements, TrustLevel,
    },
    CapabilityManager, CapabilityToken, *,
};
use aura_types::{AccountId, AccountIdExt, DeviceId, DeviceIdExt};

/// Test deterministic peer discovery
#[test]
fn test_deterministic_peer_discovery() {
    let effects = Effects::deterministic(12345, 1000000);
    let mut discovery = SocialStoragePeerDiscovery::new();

    let now = effects.now().unwrap();

    // Add peers with deterministic IDs
    for i in 0..5 {
        let peer = StoragePeer {
            peer_id: vec![i],
            device_id: DeviceId::new_with_effects(&aura_crypto::Effects::test()),
            account_id: AccountId::new_with_effects(&aura_crypto::Effects::test()),
            announcement: StorageCapabilityAnnouncement::new(
                (i as u64 + 1) * 1_000_000_000,
                TrustLevel::Medium,
                4 * 1024 * 1024,
            ),
            relationship_established_at: now,
            trust_score: 0.5 + (i as f64 * 0.1),
            storage_metrics: StorageMetrics::new(),
        };
        discovery.add_peer(peer);
    }

    // Selection should be deterministic
    let requirements = StorageRequirements::basic(1_000_000_000);
    let selected1 = discovery.select_peers(&requirements, 3);
    let selected2 = discovery.select_peers(&requirements, 3);

    assert_eq!(selected1.len(), selected2.len());
    for (p1, p2) in selected1.iter().zip(selected2.iter()) {
        assert_eq!(p1.peer_id, p2.peer_id);
    }
}

/// Test key rotation with deterministic time
#[test]
fn test_key_rotation_deterministic() {
    use aura_crypto::{KeyRotationCoordinator, KeyVersionTracker};

    let effects = Effects::deterministic(54321, 2000000);
    let mut coordinator = KeyRotationCoordinator::new();

    let _device_id = DeviceId::new_with_effects(&aura_crypto::Effects::test());
    let rel_id = vec![4, 5, 6];

    // Rotate at deterministic time
    let timestamp1 = effects.now().unwrap();
    let (event1, _spec1) = coordinator.rotate_relationship_keys(rel_id.clone(), timestamp1);

    // Same timestamp should give same results
    let mut coordinator2 = KeyRotationCoordinator::with_tracker(KeyVersionTracker::new());
    let (event2, _spec2) = coordinator2.rotate_relationship_keys(rel_id.clone(), timestamp1);

    // Events should match
    assert_eq!(format!("{:?}", event1), format!("{:?}", event2));
}

/// Test Byzantine fault injection
#[test]
fn test_byzantine_peer_handling() {
    let mut discovery = SocialStoragePeerDiscovery::new();

    // Add honest peer
    let honest_peer = StoragePeer {
        peer_id: vec![1],
        device_id: DeviceId::new_with_effects(&aura_crypto::Effects::test()),
        account_id: AccountId::new_with_effects(&aura_crypto::Effects::test()),
        announcement: StorageCapabilityAnnouncement::new(
            1_000_000_000,
            TrustLevel::High,
            4 * 1024 * 1024,
        ),
        relationship_established_at: 1000,
        trust_score: 0.9,
        storage_metrics: StorageMetrics::new(),
    };

    // Add Byzantine peer (low reliability)
    let mut byzantine_metrics = StorageMetrics::new();
    byzantine_metrics.total_chunks_stored = 100;
    byzantine_metrics.failed_stores = 90; // 90% failure rate

    let byzantine_peer = StoragePeer {
        peer_id: vec![2],
        device_id: DeviceId::new_with_effects(&aura_crypto::Effects::test()),
        account_id: AccountId::new_with_effects(&aura_crypto::Effects::test()),
        announcement: StorageCapabilityAnnouncement::new(
            2_000_000_000,
            TrustLevel::High,
            4 * 1024 * 1024,
        ),
        relationship_established_at: 1000,
        trust_score: 0.9,
        storage_metrics: byzantine_metrics,
    };

    discovery.add_peer(honest_peer);
    discovery.add_peer(byzantine_peer);

    // Selection should prefer honest peer despite Byzantine having more capacity
    let requirements = StorageRequirements::basic(500_000_000);
    let selected = discovery.select_peers(&requirements, 1);

    assert_eq!(selected.len(), 1);
    // Honest peer should be selected (peer_id = 1)
    assert_eq!(selected[0].peer_id, vec![1]);
}

/// Test capability expiration handling
#[test]
fn test_capability_expiration_deterministic() {
    let effects = Effects::deterministic(99999, 3000000);
    let _manager = CapabilityManager::new();

    let device_id = DeviceId::new_with_effects(&aura_crypto::Effects::test());
    let signature = ThresholdSignature {
        threshold: 1,
        signature_shares: vec![SignatureShare {
            device_id: DeviceId::new_with_effects(&aura_crypto::Effects::test()),
            share: vec![0; 32],
        }],
    };

    let now = effects.now().unwrap();

    // Grant capability with expiration
    let token = CapabilityToken::new(
        device_id.clone(),
        vec![Permission {
            operation: StorageOperation::Write,
            resource: ResourceScope::Public,
            grant_time: now,
            expiry: None,
        }],
        signature,
        now,
    )
    .with_expiration(now + 1000);

    // Fast-forward time
    let future_time = now + 2000;

    // Token should be expired
    assert!(token.is_expired(future_time));
}

/// Test error recovery strategies
#[test]
fn test_error_recovery_strategies() {
    // Note: IntegrationError and RecoveryStrategy types may not exist yet
    // This test demonstrates the concept of error recovery strategies
    // Skipping actual error type creation for now
}

/// Test coordinated revocation scenario
#[test]
fn test_coordinated_revocation_scenario() {
    use aura_crypto::KeyRotationCoordinator;

    let effects = Effects::deterministic(11111, 4000000);
    let mut coordinator = KeyRotationCoordinator::new();

    let device_id = DeviceId::new_with_effects(&aura_crypto::Effects::test());
    let rel_id1 = vec![4, 5, 6];
    let rel_id2 = vec![7, 8, 9];

    let now = effects.now().unwrap();

    // Establish initial state
    coordinator.rotate_relationship_keys(rel_id1.clone(), now);
    coordinator.rotate_relationship_keys(rel_id2.clone(), now);
    coordinator.rotate_storage_keys(device_id.to_bytes().unwrap(), now);

    let initial_rel1_version = coordinator.tracker().get_relationship_version(&rel_id1);
    let initial_rel2_version = coordinator.tracker().get_relationship_version(&rel_id2);
    let initial_storage_version = coordinator.tracker().get_storage_version();

    // Trigger coordinated revocation
    let (_event, new_specs) =
        coordinator.coordinated_revocation(device_id.to_bytes().unwrap(), now);

    // All versions should be incremented
    assert_eq!(
        coordinator.tracker().get_relationship_version(&rel_id1),
        initial_rel1_version + 1
    );
    assert_eq!(
        coordinator.tracker().get_relationship_version(&rel_id2),
        initial_rel2_version + 1
    );
    assert_eq!(
        coordinator.tracker().get_storage_version(),
        initial_storage_version + 1
    );

    // Should have specs for all rotated keys
    assert!(new_specs.len() >= 3);
}

/// Test storage metrics update determinism
#[test]
fn test_storage_metrics_deterministic() {
    let mut metrics1 = StorageMetrics::new();
    let mut metrics2 = StorageMetrics::new();

    // Same operations in same order
    let operations = vec![(100, true), (150, true), (200, false), (120, true)];

    for (latency, success) in &operations {
        metrics1.record_store(*latency, *success);
        metrics2.record_store(*latency, *success);
    }

    assert_eq!(metrics1.total_chunks_stored, metrics2.total_chunks_stored);
    assert_eq!(metrics1.failed_stores, metrics2.failed_stores);
    assert_eq!(metrics1.avg_store_latency_ms, metrics2.avg_store_latency_ms);
    assert_eq!(metrics1.reliability_score(), metrics2.reliability_score());
}

/// Test peer suitability scoring consistency
#[test]
fn test_peer_suitability_scoring_consistent() {
    let effects = Effects::deterministic(77777, 5000000);

    let now = effects.now().unwrap();

    let peer = StoragePeer {
        peer_id: vec![1],
        device_id: DeviceId::new_with_effects(&aura_crypto::Effects::test()),
        account_id: AccountId::new_with_effects(&aura_crypto::Effects::test()),
        announcement: StorageCapabilityAnnouncement::new(
            1_000_000_000,
            TrustLevel::High,
            4 * 1024 * 1024,
        ),
        relationship_established_at: now,
        trust_score: 0.85,
        storage_metrics: {
            let mut m = StorageMetrics::new();
            m.total_chunks_stored = 100;
            m.total_chunks_retrieved = 100;
            m.avg_store_latency_ms = 150;
            m.avg_retrieve_latency_ms = 100;
            m
        },
    };

    let requirements = StorageRequirements::basic(500_000_000);

    // Score should be consistent
    let score1 = peer.suitability_score(&requirements);
    let score2 = peer.suitability_score(&requirements);

    assert_eq!(score1, score2);
    assert!(score1 > 0.5); // Should be a good peer
}

/// Test rapid key rotation
#[test]
fn test_rapid_key_rotation() {
    use aura_crypto::KeyRotationCoordinator;

    let effects = Effects::deterministic(33333, 6000000);
    let mut coordinator = KeyRotationCoordinator::new();

    let rel_id = vec![1, 2, 3];

    let base_time = effects.now().unwrap();

    // Rotate rapidly
    for i in 0..10 {
        coordinator.rotate_relationship_keys(rel_id.clone(), base_time + i * 100);
    }

    // Version should be 10
    assert_eq!(coordinator.tracker().get_relationship_version(&rel_id), 10);

    // Old versions should not be valid
    for version in 0..10 {
        assert!(!coordinator.verify_relationship_version(&rel_id, version));
    }

    // Current version should be valid
    assert!(coordinator.verify_relationship_version(&rel_id, 10));
}

/// Test peer discovery with no suitable peers
#[test]
fn test_peer_discovery_no_suitable_peers() {
    let mut discovery = SocialStoragePeerDiscovery::new();

    // Add only low-capacity peers
    for i in 0..3 {
        let peer = StoragePeer {
            peer_id: vec![i],
            device_id: DeviceId::new_with_effects(&aura_crypto::Effects::test()),
            account_id: AccountId::new_with_effects(&aura_crypto::Effects::test()),
            announcement: StorageCapabilityAnnouncement::new(
                100_000_000, // Only 100MB
                TrustLevel::Low,
                1 * 1024 * 1024,
            ),
            relationship_established_at: 1000,
            trust_score: 0.5,
            storage_metrics: StorageMetrics::new(),
        };
        discovery.add_peer(peer);
    }

    // Require high capacity
    let requirements = StorageRequirements::basic(1_000_000_000); // Need 1GB
    let selected = discovery.select_peers(&requirements, 10);

    // Should find no suitable peers
    assert_eq!(selected.len(), 0);
}
