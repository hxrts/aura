//! Scaling behavior benchmarks for aura-sync protocols
//!
//! Measures how protocols scale with increasing numbers of peers,
//! operations, and concurrent sessions to identify performance bottlenecks.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use aura_core::effects::{JournalEffects, NetworkEffects, RandomEffects, TimeEffects};
use aura_core::{AuraError, DeviceId, FactValue, Journal};
use aura_sync::core::MetricsCollector;
use aura_sync::protocols::{
    AntiEntropyConfig, AntiEntropyProtocol, EpochConfig, EpochRotationCoordinator,
    JournalSyncConfig, JournalSyncProtocol, OTAConfig, OTAProtocol, SnapshotConfig,
    SnapshotProtocol,
};

// =============================================================================
// Scalable Test Effects System
// =============================================================================

#[derive(Debug, Clone)]
pub struct ScalableTestEffects {
    journals: Arc<Mutex<HashMap<DeviceId, Journal>>>,
    network_messages: Arc<Mutex<HashMap<DeviceId, Vec<Vec<u8>>>>>,
    metrics: Arc<MetricsCollector>,
    current_time: Arc<Mutex<u64>>,
    processing_delay: Duration,
    peer_count: usize,
}

impl ScalableTestEffects {
    pub fn new(peer_count: usize, processing_delay: Duration) -> Self {
        Self {
            journals: Arc::new(Mutex::new(HashMap::new())),
            network_messages: Arc::new(Mutex::new(HashMap::new())),
            metrics: Arc::new(MetricsCollector::new()),
            current_time: Arc::new(Mutex::new(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
            )),
            processing_delay,
            peer_count,
        }
    }

    pub fn with_journal(&self, device_id: DeviceId, journal: Journal) {
        self.journals.lock().unwrap().insert(device_id, journal);
    }

    pub fn simulate_peer_network(&self, device_id: DeviceId, peers: &[DeviceId]) {
        let mut messages = self.network_messages.lock().unwrap();
        for peer in peers {
            messages.entry(*peer).or_insert_with(Vec::new);
        }
        messages.entry(device_id).or_insert_with(Vec::new);
    }

    pub fn get_total_messages(&self) -> usize {
        self.network_messages
            .lock()
            .unwrap()
            .values()
            .map(|msgs| msgs.len())
            .sum()
    }

    pub fn clear_messages(&self) {
        self.network_messages.lock().unwrap().clear();
    }
}

impl JournalEffects for ScalableTestEffects {
    async fn get_journal(&self) -> Result<Journal, AuraError> {
        let device_id = DeviceId::new();
        self.journals
            .lock()
            .unwrap()
            .get(&device_id)
            .cloned()
            .ok_or_else(|| AuraError::Storage("No journal found".to_string()))
    }

    async fn update_journal(&self, journal: Journal) -> Result<(), AuraError> {
        let device_id = DeviceId::new();
        self.journals.lock().unwrap().insert(device_id, journal);
        Ok(())
    }
}

impl NetworkEffects for ScalableTestEffects {
    async fn send_message(&self, peer: DeviceId, data: Vec<u8>) -> Result<(), AuraError> {
        // Simulate processing delay that scales with peer count
        let delay = self.processing_delay
            + Duration::from_nanos(
                (self.processing_delay.as_nanos() * self.peer_count as u128) / 100,
            );
        tokio::time::sleep(delay).await;

        self.network_messages
            .lock()
            .unwrap()
            .entry(peer)
            .or_insert_with(Vec::new)
            .push(data);

        Ok(())
    }

    async fn receive_message(&self, _timeout: Duration) -> Result<(DeviceId, Vec<u8>), AuraError> {
        let mut messages = self.network_messages.lock().unwrap();
        for (peer, peer_messages) in messages.iter_mut() {
            if !peer_messages.is_empty() {
                let data = peer_messages.remove(0);
                return Ok((*peer, data));
            }
        }
        Err(AuraError::Network("No messages available".to_string()))
    }

    async fn broadcast_message(
        &self,
        peers: Vec<DeviceId>,
        data: Vec<u8>,
    ) -> Result<(), AuraError> {
        // Simulate broadcast overhead that scales with peer count
        let broadcast_delay = Duration::from_micros(peers.len() as u64 * 50);
        tokio::time::sleep(broadcast_delay).await;

        for peer in peers {
            self.send_message(peer, data.clone()).await?;
        }
        Ok(())
    }
}

impl TimeEffects for ScalableTestEffects {
    async fn current_time(&self) -> u64 {
        *self.current_time.lock().unwrap()
    }

    async fn sleep(&self, duration: Duration) {
        tokio::time::sleep(duration).await;
    }
}

impl RandomEffects for ScalableTestEffects {
    async fn random_bytes(&self, length: usize) -> Vec<u8> {
        (0..length).map(|i| (i % 256) as u8).collect()
    }

    async fn random_u64(&self) -> u64 {
        42 // Deterministic for scaling tests
    }
}

// =============================================================================
// Test Data Generation for Scaling
// =============================================================================

fn create_scaling_journal(base_ops: usize, scale_factor: usize) -> Journal {
    let mut journal = Journal::new();
    let total_ops = base_ops * scale_factor;

    for i in 0..total_ops {
        let key = format!("scaling_op_{}", i);
        let value = FactValue::String(format!("data_{}_{}", i, "x".repeat(50)));
        journal.facts.insert(key, value);
    }

    journal
}

fn create_peer_set(count: usize) -> Vec<DeviceId> {
    (0..count).map(|_| DeviceId::new()).collect()
}

// =============================================================================
// Peer Count Scaling Benchmarks
// =============================================================================

fn bench_anti_entropy_peer_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("anti_entropy_peer_scaling");

    for peer_count in [2, 5, 10, 25, 50, 100].iter() {
        group.throughput(Throughput::Elements(*peer_count as u64));
        group.bench_with_input(
            BenchmarkId::new("peer_count_scaling", format!("{}_peers", peer_count)),
            peer_count,
            |b, &peer_count| {
                b.to_async(tokio::runtime::Runtime::new().unwrap())
                    .iter(|| async {
                        let effects =
                            ScalableTestEffects::new(peer_count, Duration::from_micros(10));

                        let journal = create_scaling_journal(100, 1);
                        let device_id = DeviceId::new();
                        let peers = create_peer_set(peer_count);

                        effects.with_journal(device_id, journal.clone());
                        for peer in &peers {
                            effects.with_journal(*peer, journal.clone());
                        }
                        effects.simulate_peer_network(device_id, &peers);

                        let protocol = AntiEntropyProtocol::new(AntiEntropyConfig::default());

                        let start = Instant::now();

                        // Sync with each peer sequentially
                        for peer in peers {
                            let _result = protocol.execute(&effects, peer).await;
                        }

                        let total_time = start.elapsed();
                        let message_count = effects.get_total_messages();

                        black_box((total_time, message_count))
                    });
            },
        );
    }

    group.finish();
}

fn bench_journal_sync_concurrent_peers(c: &mut Criterion) {
    let mut group = c.benchmark_group("journal_sync_concurrent_scaling");

    for concurrent_peers in [2, 5, 10, 20, 40].iter() {
        group.throughput(Throughput::Elements(*concurrent_peers as u64));
        group.bench_with_input(
            BenchmarkId::new(
                "concurrent_peer_sync",
                format!("{}_concurrent", concurrent_peers),
            ),
            concurrent_peers,
            |b, &concurrent_peers| {
                b.to_async(tokio::runtime::Runtime::new().unwrap())
                    .iter(|| async {
                        let effects = Arc::new(ScalableTestEffects::new(
                            concurrent_peers,
                            Duration::from_micros(5),
                        ));

                        let journal = create_scaling_journal(150, 1);
                        let device_id = DeviceId::new();
                        let peers = create_peer_set(concurrent_peers);

                        effects.with_journal(device_id, journal.clone());
                        for peer in &peers {
                            effects.with_journal(*peer, journal.clone());
                        }
                        effects.simulate_peer_network(device_id, &peers);

                        let start = Instant::now();

                        // Sync with all peers concurrently
                        let mut handles = Vec::new();
                        for chunk in peers.chunks(5) {
                            // Process in chunks to avoid overwhelming
                            let effects_clone = effects.clone();
                            let chunk_peers = chunk.to_vec();

                            let handle = tokio::spawn(async move {
                                let protocol =
                                    JournalSyncProtocol::new(JournalSyncConfig::default());
                                protocol.sync_with_peers(&*effects_clone, chunk_peers).await
                            });
                            handles.push(handle);
                        }

                        for handle in handles {
                            let _ = handle.await;
                        }

                        let total_time = start.elapsed();
                        let message_count = effects.get_total_messages();

                        black_box((total_time, message_count))
                    });
            },
        );
    }

    group.finish();
}

// =============================================================================
// Operation Count Scaling Benchmarks
// =============================================================================

fn bench_operation_count_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("operation_count_scaling");

    for op_count in [100, 500, 1000, 2500, 5000, 10000].iter() {
        group.throughput(Throughput::Elements(*op_count as u64));
        group.bench_with_input(
            BenchmarkId::new("digest_creation_scaling", format!("{}_ops", op_count)),
            op_count,
            |b, &op_count| {
                b.iter(|| {
                    let journal = create_scaling_journal(op_count, 1);
                    let protocol = AntiEntropyProtocol::new(AntiEntropyConfig::default());

                    let start = Instant::now();
                    let digest = protocol.create_digest(&journal);
                    let creation_time = start.elapsed();

                    black_box((digest, creation_time))
                });
            },
        );
    }

    group.finish();
}

fn bench_large_sync_operation_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("large_sync_scaling");
    group.sample_size(20); // Reduce samples for expensive tests

    for scale_factor in [1, 2, 5, 10].iter() {
        let total_ops = 1000 * scale_factor;
        group.throughput(Throughput::Elements(total_ops as u64));
        group.bench_with_input(
            BenchmarkId::new("large_journal_sync", format!("{}x_scale", scale_factor)),
            scale_factor,
            |b, &scale_factor| {
                b.to_async(tokio::runtime::Runtime::new().unwrap())
                    .iter(|| async {
                        let effects = ScalableTestEffects::new(5, Duration::from_micros(1));

                        let journal1 = create_scaling_journal(1000, scale_factor);
                        let journal2 = create_scaling_journal(800, scale_factor); // Different sizes
                        let device_id = DeviceId::new();
                        let peer_id = DeviceId::new();

                        effects.with_journal(device_id, journal1);
                        effects.with_journal(peer_id, journal2);

                        let protocol = AntiEntropyProtocol::new(AntiEntropyConfig::default());

                        let start = Instant::now();
                        let _result = protocol.execute(&effects, peer_id).await;
                        let sync_time = start.elapsed();

                        black_box(sync_time)
                    });
            },
        );
    }

    group.finish();
}

// =============================================================================
// Concurrent Session Scaling Benchmarks
// =============================================================================

fn bench_concurrent_session_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("concurrent_session_scaling");

    for session_count in [1, 3, 5, 10, 15].iter() {
        group.bench_with_input(
            BenchmarkId::new(
                "concurrent_sync_sessions",
                format!("{}_sessions", session_count),
            ),
            session_count,
            |b, &session_count| {
                b.to_async(tokio::runtime::Runtime::new().unwrap())
                    .iter(|| async {
                        let effects = Arc::new(ScalableTestEffects::new(
                            10, // Fixed peer count
                            Duration::from_micros(20),
                        ));

                        let start = Instant::now();
                        let mut handles = Vec::new();

                        for session_id in 0..session_count {
                            let effects_clone = effects.clone();

                            let handle = tokio::spawn(async move {
                                let journal = create_scaling_journal(200, 1);
                                let device_id = DeviceId::new();
                                let peers = create_peer_set(3);

                                effects_clone.with_journal(device_id, journal.clone());
                                for peer in &peers {
                                    effects_clone.with_journal(*peer, journal.clone());
                                }

                                let protocol =
                                    JournalSyncProtocol::new(JournalSyncConfig::default());
                                protocol.sync_with_peers(&*effects_clone, peers).await
                            });
                            handles.push(handle);
                        }

                        for handle in handles {
                            let _ = handle.await;
                        }

                        let total_time = start.elapsed();
                        let message_count = effects.get_total_messages();

                        black_box((total_time, message_count))
                    });
            },
        );
    }

    group.finish();
}

// =============================================================================
// Snapshot Protocol Scaling Benchmarks
// =============================================================================

fn bench_snapshot_coordination_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("snapshot_coordination_scaling");

    for participant_count in [3, 5, 10, 15, 25].iter() {
        group.bench_with_input(
            BenchmarkId::new(
                "snapshot_participants",
                format!("{}_participants", participant_count),
            ),
            participant_count,
            |b, &participant_count| {
                b.to_async(tokio::runtime::Runtime::new().unwrap())
                    .iter(|| async {
                        let effects =
                            ScalableTestEffects::new(participant_count, Duration::from_micros(100));

                        let journal = create_scaling_journal(1000, 1);
                        let device_id = DeviceId::new();
                        let participants = create_peer_set(participant_count);

                        effects.with_journal(device_id, journal.clone());
                        for participant in &participants {
                            effects.with_journal(*participant, journal.clone());
                        }
                        effects.simulate_peer_network(device_id, &participants);

                        let protocol = SnapshotProtocol::new(SnapshotConfig::default());

                        let start = Instant::now();
                        let _result = protocol.coordinate_snapshot(&effects, participants).await;
                        let coordination_time = start.elapsed();

                        black_box(coordination_time)
                    });
            },
        );
    }

    group.finish();
}

// =============================================================================
// OTA Distribution Scaling Benchmarks
// =============================================================================

fn bench_ota_distribution_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("ota_distribution_scaling");

    for node_count in [5, 10, 20, 30, 50].iter() {
        group.throughput(Throughput::Elements(*node_count as u64));
        group.bench_with_input(
            BenchmarkId::new("ota_distribution", format!("{}_nodes", node_count)),
            node_count,
            |b, &node_count| {
                b.to_async(tokio::runtime::Runtime::new().unwrap())
                    .iter(|| async {
                        let effects =
                            ScalableTestEffects::new(node_count, Duration::from_micros(200));

                        let update_data = vec![0u8; 1024 * 1024]; // 1MB update
                        let nodes = create_peer_set(node_count);

                        for node in &nodes {
                            effects.with_journal(*node, Journal::new());
                        }

                        let protocol = OTAProtocol::new(OTAConfig::default());

                        let start = Instant::now();
                        let _result = protocol
                            .distribute_update(&effects, nodes, update_data)
                            .await;
                        let distribution_time = start.elapsed();

                        black_box(distribution_time)
                    });
            },
        );
    }

    group.finish();
}

// =============================================================================
// Epoch Coordination Scaling Benchmarks
// =============================================================================

fn bench_epoch_rotation_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("epoch_rotation_scaling");

    for participant_count in [3, 5, 10, 20, 30].iter() {
        group.bench_with_input(
            BenchmarkId::new(
                "epoch_participants",
                format!("{}_participants", participant_count),
            ),
            participant_count,
            |b, &participant_count| {
                b.iter(|| {
                    let device_id = DeviceId::new();
                    let config = EpochConfig::default();
                    let mut coordinator = EpochRotationCoordinator::new(device_id, 0, config);

                    let participants = create_peer_set(participant_count);
                    let context_id = aura_core::ContextId::new();

                    let start = Instant::now();

                    // Initiate rotation
                    let rotation_id = coordinator
                        .initiate_rotation(participants.clone(), context_id)
                        .unwrap();

                    // Process confirmations from all participants
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

                    // Commit and cleanup
                    let _ = coordinator.commit_rotation(&rotation_id);
                    let _ = coordinator.cleanup_completed_rotations();

                    let total_time = start.elapsed();

                    black_box(total_time)
                });
            },
        );
    }

    group.finish();
}

// =============================================================================
// Combined Scaling Stress Tests
// =============================================================================

fn bench_combined_scaling_stress_test(c: &mut Criterion) {
    let mut group = c.benchmark_group("combined_scaling_stress");
    group.sample_size(10); // Very few samples for stress tests

    for complexity_level in [1, 2, 3].iter() {
        let peer_count = 5 * complexity_level;
        let op_count = 500 * complexity_level;
        let session_count = 2 * complexity_level;

        group.bench_with_input(
            BenchmarkId::new("full_system_stress", format!("level_{}", complexity_level)),
            complexity_level,
            |b, &_complexity_level| {
                b.to_async(tokio::runtime::Runtime::new().unwrap())
                    .iter(|| async {
                        let effects = Arc::new(ScalableTestEffects::new(
                            peer_count,
                            Duration::from_micros(50),
                        ));

                        let start = Instant::now();
                        let mut all_handles = Vec::new();

                        // Run multiple concurrent sync sessions
                        for session in 0..session_count {
                            let effects_clone = effects.clone();

                            let handle = tokio::spawn(async move {
                                let journal = create_scaling_journal(op_count, 1);
                                let device_id = DeviceId::new();
                                let peers = create_peer_set(peer_count);

                                effects_clone.with_journal(device_id, journal.clone());
                                for peer in &peers {
                                    effects_clone.with_journal(*peer, journal.clone());
                                }

                                // Mix different protocols
                                if session % 3 == 0 {
                                    // Anti-entropy sync
                                    let protocol =
                                        AntiEntropyProtocol::new(AntiEntropyConfig::default());
                                    for peer in peers {
                                        let _ = protocol.execute(&*effects_clone, peer).await;
                                    }
                                } else if session % 3 == 1 {
                                    // Journal sync
                                    let protocol =
                                        JournalSyncProtocol::new(JournalSyncConfig::default());
                                    let _ = protocol.sync_with_peers(&*effects_clone, peers).await;
                                } else {
                                    // Snapshot coordination
                                    let protocol = SnapshotProtocol::new(SnapshotConfig::default());
                                    let _ =
                                        protocol.coordinate_snapshot(&*effects_clone, peers).await;
                                }
                            });
                            all_handles.push(handle);
                        }

                        for handle in all_handles {
                            let _ = handle.await;
                        }

                        let total_time = start.elapsed();
                        let final_message_count = effects.get_total_messages();

                        black_box((total_time, final_message_count))
                    });
            },
        );
    }

    group.finish();
}

// =============================================================================
// Criterion Configuration
// =============================================================================

criterion_group!(
    scaling_behavior_benches,
    bench_anti_entropy_peer_scaling,
    bench_journal_sync_concurrent_peers,
    bench_operation_count_scaling,
    bench_large_sync_operation_scaling,
    bench_concurrent_session_scaling,
    bench_snapshot_coordination_scaling,
    bench_ota_distribution_scaling,
    bench_epoch_rotation_scaling,
    bench_combined_scaling_stress_test,
);

criterion_main!(scaling_behavior_benches);
