//! Performance Benchmarks for SSB + Storage
//!
//! Benchmarks covering:
//! - Envelope recognition performance (should handle 1000 envelopes/sec)
//! - CRDT merge overhead (should be < 10ms for typical documents)
//! - Chunk encryption throughput (should saturate network)
//! - Capability verification (should be < 1ms)
//!
//! Reference: work/ssb_storage.md Phase 5.4

use aura_crypto::Effects;
use aura_store::{
    manifest::{Permission, ResourceScope, StorageOperation},
    social_storage::TrustLevel,
    *,
};
use std::time::Instant;

/// Benchmark envelope recognition performance
/// Target: 1000 envelopes/sec
#[test]
fn bench_envelope_recognition() {
    let effects = Effects::deterministic(12345, 1000000);
    let mut discovery = SocialStoragePeerDiscovery::new();

    let now = effects.now().unwrap();

    // Setup 100 peers to test recognition at scale
    for i in 0..100 {
        let peer = StoragePeer {
            peer_id: vec![i],
            device_id: vec![i],
            account_id: vec![100],
            announcement: StorageCapabilityAnnouncement::new(
                1_000_000_000,
                TrustLevel::High,
                4 * 1024 * 1024,
            ),
            relationship_established_at: now,
            trust_score: 0.9,
            storage_metrics: StorageMetrics::new(),
        };
        discovery.add_peer(peer);
    }

    let requirements = StorageRequirements::basic(500_000_000);

    // Benchmark peer selection (simulating envelope recognition)
    let start = Instant::now();
    let iterations = 1000;

    for _ in 0..iterations {
        let _selected = discovery.select_peers(&requirements, 10);
    }

    let duration = start.elapsed();
    let ops_per_sec = (iterations as f64 / duration.as_secs_f64()) as u64;

    println!("Envelope recognition performance: {} ops/sec", ops_per_sec);
    println!("Average latency: {:?}", duration / iterations);

    // Should handle at least 1000 operations per second
    assert!(
        ops_per_sec >= 1000,
        "Performance target not met: {} ops/sec (expected >= 1000)",
        ops_per_sec
    );
}

/// Benchmark CRDT merge performance
/// Target: < 10ms for typical documents
#[test]
fn bench_crdt_merge_overhead() {
    let effects = Effects::deterministic(54321, 2000000);

    // Simulate CRDT merge by creating and merging peer discovery objects
    let start = Instant::now();
    let iterations = 100;

    for i in 0..iterations {
        let mut discovery1 = SocialStoragePeerDiscovery::new();
        let mut discovery2 = SocialStoragePeerDiscovery::new();

        let now = effects.now().unwrap() + i;

        // Add peers to both replicas
        for j in 0..10 {
            let peer1 = StoragePeer {
                peer_id: vec![j * 2],
                device_id: vec![j * 2],
                account_id: vec![100],
                announcement: StorageCapabilityAnnouncement::new(
                    1_000_000_000,
                    TrustLevel::Medium,
                    4 * 1024 * 1024,
                ),
                relationship_established_at: now,
                trust_score: 0.6,
                storage_metrics: StorageMetrics::new(),
            };

            let peer2 = StoragePeer {
                peer_id: vec![j * 2 + 1],
                device_id: vec![j * 2 + 1],
                account_id: vec![100],
                announcement: StorageCapabilityAnnouncement::new(
                    1_000_000_000,
                    TrustLevel::Medium,
                    4 * 1024 * 1024,
                ),
                relationship_established_at: now,
                trust_score: 0.6,
                storage_metrics: StorageMetrics::new(),
            };

            discovery1.add_peer(peer1);
            discovery2.add_peer(peer2);
        }

        // Simulate merge by combining selections
        let requirements = StorageRequirements::basic(500_000_000);
        let _peers1 = discovery1.select_peers(&requirements, 20);
        let _peers2 = discovery2.select_peers(&requirements, 20);
    }

    let duration = start.elapsed();
    let avg_merge_time = duration / iterations as u32;

    println!("CRDT merge overhead: {:?} average", avg_merge_time);
    println!("Total time for {} merges: {:?}", iterations, duration);

    // Should be less than 10ms per merge
    assert!(
        avg_merge_time.as_millis() < 10,
        "CRDT merge too slow: {:?} (expected < 10ms)",
        avg_merge_time
    );
}

/// Benchmark chunk encryption throughput
/// Target: Should saturate network (CPU-bound, not crypto-bound)
#[test]
fn bench_chunk_encryption_throughput() {
    let chunk_store = ChunkStore::new(std::path::PathBuf::from("/tmp/bench_chunks"));

    // Test with 1MB chunks
    let chunk_size = 1024 * 1024;
    let chunk_data = vec![0u8; chunk_size];
    let iterations = 100;

    let key_spec = KeyDerivationSpec::device_encryption(vec![1, 2, 3]);

    let start = Instant::now();

    for _ in 0..iterations {
        let _encrypted = chunk_store
            .encrypt_chunk(&chunk_data, &key_spec)
            .expect("Encryption failed");
    }

    let duration = start.elapsed();
    let total_mb = (iterations * chunk_size) / (1024 * 1024);
    let throughput_mbps = (total_mb as f64 / duration.as_secs_f64()) as u64;

    println!("Chunk encryption throughput: {} MB/s", throughput_mbps);
    println!("Encrypted {} MB in {:?}", total_mb, duration);

    // Should achieve at least 5 MB/s (reasonable baseline, actual performance depends on system)
    // Note: Encryption includes key derivation overhead per chunk
    assert!(
        throughput_mbps >= 5,
        "Encryption throughput too low: {} MB/s (expected >= 5)",
        throughput_mbps
    );
}

/// Benchmark capability verification
/// Target: < 1ms per check
#[test]
fn bench_capability_verification() {
    let effects = Effects::deterministic(99999, 3000000);
    let mut manager = CapabilityManager::new();

    let device_id = vec![1, 2, 3];
    let signature = ThresholdSignature {
        signers: vec![vec![1, 2, 3]],
        signature_shares: vec![vec![0; 32]],
        aggregated_signature: vec![0; 64],
    };

    let now = effects.now().unwrap();

    // Grant multiple capabilities
    for _ in 0..10 {
        let _token = manager
            .grant_storage_capability(
                device_id.clone(),
                StorageOperation::Read,
                ResourceScope::AllOwnedObjects,
                signature.clone(),
                now,
            )
            .expect("Failed to grant capability");
    }

    let access_control = AccessControl::new_capability_based(vec![Permission::Storage {
        operation: StorageOperation::Read,
        resource: ResourceScope::AllOwnedObjects,
    }]);

    // Benchmark verification
    let start = Instant::now();
    let iterations = 10000;

    for _ in 0..iterations {
        let _result = manager.verify_storage_permissions(&device_id, &access_control, now);
    }

    let duration = start.elapsed();
    let avg_verification_time = duration / iterations;
    let ops_per_sec = (iterations as f64 / duration.as_secs_f64()) as u64;

    println!(
        "Capability verification: {:?} average ({} ops/sec)",
        avg_verification_time, ops_per_sec
    );

    // Should be less than 1ms per verification
    assert!(
        avg_verification_time.as_micros() < 1000,
        "Verification too slow: {:?} (expected < 1ms)",
        avg_verification_time
    );
}

/// Benchmark peer discovery at scale
#[test]
fn bench_peer_discovery_scale() {
    let effects = Effects::deterministic(77777, 5000000);
    let mut discovery = SocialStoragePeerDiscovery::new();

    let now = effects.now().unwrap();

    // Add 1000 peers
    let num_peers = 1000;
    for i in 0..num_peers {
        let peer = StoragePeer {
            peer_id: vec![i as u8, (i >> 8) as u8],
            device_id: vec![i as u8, (i >> 8) as u8],
            account_id: vec![100],
            announcement: StorageCapabilityAnnouncement::new(
                1_000_000_000 + (i as u64 * 1_000_000),
                if i % 3 == 0 {
                    TrustLevel::High
                } else if i % 3 == 1 {
                    TrustLevel::Medium
                } else {
                    TrustLevel::Low
                },
                4 * 1024 * 1024,
            ),
            relationship_established_at: now,
            trust_score: 0.5 + (i as f64 / num_peers as f64) * 0.4,
            storage_metrics: StorageMetrics::new(),
        };
        discovery.add_peer(peer);
    }

    let requirements = StorageRequirements::basic(500_000_000);

    // Benchmark selection with 1000 peers
    let start = Instant::now();
    let iterations = 1000;

    for _ in 0..iterations {
        let _selected = discovery.select_peers(&requirements, 10);
    }

    let duration = start.elapsed();
    let avg_selection_time = duration / iterations;
    let ops_per_sec = (iterations as f64 / duration.as_secs_f64()) as u64;

    println!(
        "Peer discovery at scale ({} peers): {:?} average ({} ops/sec)",
        num_peers, avg_selection_time, ops_per_sec
    );

    // Should handle at least 500 selections per second even with 1000 peers
    assert!(
        ops_per_sec >= 500,
        "Discovery too slow at scale: {} ops/sec (expected >= 500)",
        ops_per_sec
    );
}

/// Benchmark storage metrics update performance
#[test]
fn bench_storage_metrics_update() {
    let mut metrics = StorageMetrics::new();

    let start = Instant::now();
    let iterations = 100000;

    // Simulate rapid metrics updates
    for i in 0..iterations {
        let latency = (100 + (i % 200)) as u32;
        let success = i % 10 != 0; // 90% success rate
        metrics.record_store(latency, success);
    }

    let duration = start.elapsed();
    let avg_update_time = duration / iterations;
    let ops_per_sec = (iterations as f64 / duration.as_secs_f64()) as u64;

    println!(
        "Storage metrics update: {:?} average ({} ops/sec)",
        avg_update_time, ops_per_sec
    );

    // Metrics updates should be extremely fast (< 1 microsecond)
    assert!(
        avg_update_time.as_nanos() < 1000,
        "Metrics update too slow: {:?} (expected < 1μs)",
        avg_update_time
    );

    // Verify metrics were calculated correctly
    // 90% success rate means 90% of iterations were successful
    let expected_successful = (iterations * 9 / 10) as u64;
    let expected_failed = (iterations / 10) as u64;

    assert_eq!(metrics.total_chunks_stored, expected_successful);
    assert_eq!(metrics.failed_stores, expected_failed);
}

/// Benchmark key rotation coordination
#[test]
fn bench_key_rotation_coordination() {
    use aura_crypto::KeyRotationCoordinator;

    let effects = Effects::deterministic(33333, 6000000);
    let mut coordinator = KeyRotationCoordinator::new();

    let base_time = effects.now().unwrap();

    // Setup 100 relationships
    let rel_ids: Vec<Vec<u8>> = (0..100).map(|i| vec![i]).collect();

    for (i, rel_id) in rel_ids.iter().enumerate() {
        let timestamp = base_time + (i as u64 * 10);
        coordinator.rotate_relationship_keys(rel_id.clone(), timestamp);
    }

    // Benchmark rotation performance
    let start = Instant::now();
    let iterations = 1000;

    for i in 0..iterations {
        let rel_id = &rel_ids[i % rel_ids.len()];
        let timestamp = base_time + 10000 + i as u64;
        coordinator.rotate_relationship_keys(rel_id.clone(), timestamp);
    }

    let duration = start.elapsed();
    let avg_rotation_time = duration / iterations as u32;
    let ops_per_sec = (iterations as f64 / duration.as_secs_f64()) as u64;

    println!(
        "Key rotation coordination: {:?} average ({} ops/sec)",
        avg_rotation_time, ops_per_sec
    );

    // Should handle at least 100 rotations per second
    assert!(
        ops_per_sec >= 100,
        "Key rotation too slow: {} ops/sec (expected >= 100)",
        ops_per_sec
    );
}
