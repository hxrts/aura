//! Protocol latency benchmarks for aura-sync
//!
//! Measures end-to-end latency for different protocol operations
//! under various network conditions and peer configurations.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::time::timeout;

use aura_core::effects::{JournalEffects, NetworkEffects, RandomEffects, TimeEffects};
use aura_core::{AuraError, DeviceId, FactValue, Journal};
use aura_sync::protocols::{
    AntiEntropyConfig, AntiEntropyProtocol, EpochConfig, EpochRotationCoordinator,
    JournalSyncConfig, JournalSyncProtocol, ReceiptVerificationConfig, ReceiptVerificationProtocol,
};

// =============================================================================
// Latency Tracking Effects
// =============================================================================

#[derive(Debug, Clone)]
pub struct LatencyTrackingEffects {
    journals: Arc<Mutex<HashMap<DeviceId, Journal>>>,
    network_messages: Arc<Mutex<Vec<(DeviceId, Vec<u8>, Instant)>>>,
    current_time: Arc<Mutex<u64>>,
    base_latency: Duration,
    jitter_max: Duration,
    packet_loss_rate: f32,
}

impl LatencyTrackingEffects {
    pub fn new(base_latency: Duration, jitter_max: Duration, packet_loss_rate: f32) -> Self {
        Self {
            journals: Arc::new(Mutex::new(HashMap::new())),
            network_messages: Arc::new(Mutex::new(Vec::new())),
            current_time: Arc::new(Mutex::new(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
            )),
            base_latency,
            jitter_max,
            packet_loss_rate,
        }
    }

    pub fn with_journal(&self, device_id: DeviceId, journal: Journal) {
        self.journals.lock().unwrap().insert(device_id, journal);
    }

    fn simulate_network_conditions(&self) -> Duration {
        // Simple jitter simulation
        let jitter_ms = (rand::random::<f32>() * self.jitter_max.as_millis() as f32) as u64;
        self.base_latency + Duration::from_millis(jitter_ms)
    }

    fn should_drop_packet(&self) -> bool {
        rand::random::<f32>() < self.packet_loss_rate
    }

    pub fn get_average_latency(&self) -> Duration {
        let messages = self.network_messages.lock().unwrap();
        if messages.is_empty() {
            return Duration::ZERO;
        }

        let total_latency: Duration = messages
            .iter()
            .map(|(_, _, sent_time)| sent_time.elapsed())
            .sum();

        total_latency / messages.len() as u32
    }

    pub fn get_latency_percentile(&self, percentile: f32) -> Duration {
        let mut latencies: Vec<Duration> = self
            .network_messages
            .lock()
            .unwrap()
            .iter()
            .map(|(_, _, sent_time)| sent_time.elapsed())
            .collect();

        if latencies.is_empty() {
            return Duration::ZERO;
        }

        latencies.sort();
        let index =
            ((latencies.len() as f32 * percentile / 100.0) as usize).min(latencies.len() - 1);
        latencies[index]
    }

    pub fn clear_measurements(&self) {
        self.network_messages.lock().unwrap().clear();
    }
}

impl JournalEffects for LatencyTrackingEffects {
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

impl NetworkEffects for LatencyTrackingEffects {
    async fn send_message(&self, peer: DeviceId, data: Vec<u8>) -> Result<(), AuraError> {
        if self.should_drop_packet() {
            return Err(AuraError::Network("Packet dropped".to_string()));
        }

        let network_delay = self.simulate_network_conditions();
        let send_time = Instant::now();

        tokio::time::sleep(network_delay).await;

        self.network_messages
            .lock()
            .unwrap()
            .push((peer, data, send_time));

        Ok(())
    }

    async fn receive_message(
        &self,
        timeout_duration: Duration,
    ) -> Result<(DeviceId, Vec<u8>), AuraError> {
        let start = Instant::now();

        while start.elapsed() < timeout_duration {
            let mut messages = self.network_messages.lock().unwrap();
            if !messages.is_empty() {
                let (peer, data, _) = messages.remove(0);
                return Ok((peer, data));
            }
            drop(messages);

            tokio::time::sleep(Duration::from_millis(1)).await;
        }

        Err(AuraError::Network("Receive timeout".to_string()))
    }

    async fn broadcast_message(
        &self,
        peers: Vec<DeviceId>,
        data: Vec<u8>,
    ) -> Result<(), AuraError> {
        let mut errors = Vec::new();

        for peer in peers {
            if let Err(e) = self.send_message(peer, data.clone()).await {
                errors.push(e);
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(AuraError::Network(format!(
                "Broadcast failed: {} errors",
                errors.len()
            )))
        }
    }
}

impl TimeEffects for LatencyTrackingEffects {
    async fn current_time(&self) -> u64 {
        *self.current_time.lock().unwrap()
    }

    async fn sleep(&self, duration: Duration) {
        tokio::time::sleep(duration).await;
    }
}

impl RandomEffects for LatencyTrackingEffects {
    async fn random_bytes(&self, length: usize) -> Vec<u8> {
        (0..length).map(|i| (i % 256) as u8).collect()
    }

    async fn random_u64(&self) -> u64 {
        rand::random()
    }
}

// =============================================================================
// Test Data Utilities
// =============================================================================

fn create_test_journal_with_size(op_count: usize, payload_size: usize) -> Journal {
    let mut journal = Journal::new();

    for i in 0..op_count {
        let key = format!("latency_test_op_{}", i);
        let payload = "x".repeat(payload_size);
        let value = FactValue::String(format!("{}_payload_{}", payload, i));
        journal.facts.insert(key, value);
    }

    journal
}

// =============================================================================
// Anti-Entropy Protocol Latency Benchmarks
// =============================================================================

fn bench_anti_entropy_round_trip_latency(c: &mut Criterion) {
    let mut group = c.benchmark_group("anti_entropy_latency");

    for latency_ms in [10, 50, 100, 250, 500].iter() {
        group.bench_with_input(
            BenchmarkId::new("round_trip_latency", format!("{}ms", latency_ms)),
            latency_ms,
            |b, &latency_ms| {
                b.to_async(tokio::runtime::Runtime::new().unwrap())
                    .iter(|| async {
                        let effects = LatencyTrackingEffects::new(
                            Duration::from_millis(latency_ms),
                            Duration::from_millis(latency_ms / 10), // 10% jitter
                            0.01,                                   // 1% packet loss
                        );

                        let journal = create_test_journal_with_size(100, 200);
                        let device_id = DeviceId::new();
                        let peer_id = DeviceId::new();

                        effects.with_journal(device_id, journal.clone());
                        effects.with_journal(peer_id, journal);

                        let protocol = AntiEntropyProtocol::new(AntiEntropyConfig::default());

                        let start = Instant::now();
                        let _result = protocol.execute(&effects, peer_id).await;
                        let total_latency = start.elapsed();

                        black_box((total_latency, effects.get_average_latency()))
                    });
            },
        );
    }

    group.finish();
}

fn bench_digest_exchange_latency(c: &mut Criterion) {
    let mut group = c.benchmark_group("digest_exchange_latency");

    for journal_size in [100, 500, 1000, 2000].iter() {
        group.throughput(Throughput::Elements(*journal_size as u64));
        group.bench_with_input(
            BenchmarkId::new("digest_creation_and_comparison", journal_size),
            journal_size,
            |b, &journal_size| {
                b.iter(|| {
                    let journal1 = create_test_journal_with_size(journal_size, 100);
                    let journal2 = create_test_journal_with_size(journal_size, 100);

                    let protocol = AntiEntropyProtocol::new(AntiEntropyConfig::default());

                    let start = Instant::now();
                    let digest1 = protocol.create_digest(&journal1);
                    let digest2 = protocol.create_digest(&journal2);
                    let _status = protocol.compare_digests(&digest1, &digest2);
                    let processing_time = start.elapsed();

                    black_box(processing_time)
                });
            },
        );
    }

    group.finish();
}

// =============================================================================
// Journal Sync Protocol Latency Benchmarks
// =============================================================================

fn bench_journal_sync_end_to_end_latency(c: &mut Criterion) {
    let mut group = c.benchmark_group("journal_sync_e2e_latency");

    for peer_count in [2, 5, 10, 20].iter() {
        group.bench_with_input(
            BenchmarkId::new("multi_peer_sync_latency", format!("{}_peers", peer_count)),
            peer_count,
            |b, &peer_count| {
                b.to_async(tokio::runtime::Runtime::new().unwrap())
                    .iter(|| async {
                        let effects = LatencyTrackingEffects::new(
                            Duration::from_millis(25),
                            Duration::from_millis(5),
                            0.02, // 2% packet loss
                        );
                        effects.clear_measurements();

                        let journal = create_test_journal_with_size(200, 150);
                        let device_id = DeviceId::new();
                        let peers: Vec<DeviceId> =
                            (0..*peer_count).map(|_| DeviceId::new()).collect();

                        effects.with_journal(device_id, journal.clone());
                        for peer in &peers {
                            effects.with_journal(*peer, journal.clone());
                        }

                        let protocol = JournalSyncProtocol::new(JournalSyncConfig::default());

                        let start = Instant::now();
                        let _result = protocol.sync_with_peers(&effects, peers).await;
                        let total_time = start.elapsed();

                        black_box((
                            total_time,
                            effects.get_average_latency(),
                            effects.get_latency_percentile(95.0),
                        ))
                    });
            },
        );
    }

    group.finish();
}

fn bench_journal_sync_message_latency(c: &mut Criterion) {
    let mut group = c.benchmark_group("journal_sync_message_latency");

    for ops_per_message in [10, 50, 100, 250].iter() {
        group.throughput(Throughput::Elements(*ops_per_message as u64));
        group.bench_with_input(
            BenchmarkId::new(
                "message_processing_latency",
                format!("{}_ops", ops_per_message),
            ),
            ops_per_message,
            |b, &ops_per_message| {
                b.to_async(tokio::runtime::Runtime::new().unwrap())
                    .iter(|| async {
                        let effects = LatencyTrackingEffects::new(
                            Duration::from_millis(10),
                            Duration::from_millis(2),
                            0.0, // No packet loss for message processing
                        );

                        let protocol = JournalSyncProtocol::new(JournalSyncConfig::default());
                        let peer = DeviceId::new();

                        // Create a large operations response message
                        let operations: Vec<Vec<u8>> = (0..ops_per_message)
                            .map(|i| format!("operation_data_{}_payload", i).into_bytes())
                            .collect();

                        let message = aura_sync::protocols::SyncMessage::OperationsResponse {
                            operations: operations.into_iter().map(|data| data.into()).collect(),
                            has_more: false,
                        };

                        let start = Instant::now();
                        let _result = protocol.handle_sync_message(&effects, peer, message).await;
                        let processing_time = start.elapsed();

                        black_box(processing_time)
                    });
            },
        );
    }

    group.finish();
}

// =============================================================================
// Network Condition Impact Benchmarks
// =============================================================================

fn bench_latency_under_packet_loss(c: &mut Criterion) {
    let mut group = c.benchmark_group("packet_loss_impact");

    for loss_rate in [0.0, 0.01, 0.05, 0.10, 0.20].iter() {
        group.bench_with_input(
            BenchmarkId::new(
                "anti_entropy_with_packet_loss",
                format!("{:.0}%", loss_rate * 100.0),
            ),
            loss_rate,
            |b, &loss_rate| {
                b.to_async(tokio::runtime::Runtime::new().unwrap())
                    .iter(|| async {
                        let effects = LatencyTrackingEffects::new(
                            Duration::from_millis(50),
                            Duration::from_millis(10),
                            loss_rate,
                        );

                        let journal = create_test_journal_with_size(100, 100);
                        let device_id = DeviceId::new();
                        let peer_id = DeviceId::new();

                        effects.with_journal(device_id, journal.clone());
                        effects.with_journal(peer_id, journal);

                        let protocol = AntiEntropyProtocol::new(AntiEntropyConfig::default());

                        let start = Instant::now();
                        let result = timeout(
                            Duration::from_secs(5), // 5 second timeout
                            protocol.execute(&effects, peer_id),
                        )
                        .await;
                        let total_time = start.elapsed();

                        black_box((total_time, result.is_ok()))
                    });
            },
        );
    }

    group.finish();
}

fn bench_jitter_impact_on_sync(c: &mut Criterion) {
    let mut group = c.benchmark_group("network_jitter_impact");

    for jitter_ratio in [0.0, 0.1, 0.25, 0.5, 1.0].iter() {
        let base_latency = Duration::from_millis(100);
        let jitter = Duration::from_millis((100.0 * jitter_ratio) as u64);

        group.bench_with_input(
            BenchmarkId::new("sync_with_jitter", format!("{:.0}%", jitter_ratio * 100.0)),
            jitter_ratio,
            |b, &_jitter_ratio| {
                b.to_async(tokio::runtime::Runtime::new().unwrap())
                    .iter(|| async {
                        let effects = LatencyTrackingEffects::new(base_latency, jitter, 0.0);

                        let journal = create_test_journal_with_size(150, 100);
                        let device_id = DeviceId::new();
                        let peers = vec![DeviceId::new(), DeviceId::new(), DeviceId::new()];

                        effects.with_journal(device_id, journal.clone());
                        for peer in &peers {
                            effects.with_journal(*peer, journal.clone());
                        }

                        let protocol = JournalSyncProtocol::new(JournalSyncConfig::default());

                        let start = Instant::now();
                        let _result = protocol.sync_with_peers(&effects, peers).await;
                        let total_time = start.elapsed();

                        black_box((
                            total_time,
                            effects.get_average_latency(),
                            effects.get_latency_percentile(99.0),
                        ))
                    });
            },
        );
    }

    group.finish();
}

// =============================================================================
// Epoch Coordination Latency Benchmarks
// =============================================================================

fn bench_epoch_rotation_latency(c: &mut Criterion) {
    let mut group = c.benchmark_group("epoch_rotation_latency");

    for participant_count in [3, 5, 10, 15].iter() {
        group.bench_with_input(
            BenchmarkId::new(
                "epoch_rotation_coordination",
                format!("{}_participants", participant_count),
            ),
            participant_count,
            |b, &participant_count| {
                b.iter(|| {
                    let device_id = DeviceId::new();
                    let config = EpochConfig::default();
                    let mut coordinator = EpochRotationCoordinator::new(device_id, 0, config);

                    let participants: Vec<DeviceId> =
                        (0..participant_count).map(|_| DeviceId::new()).collect();
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

                    // Commit rotation
                    let _ = coordinator.commit_rotation(&rotation_id);

                    let coordination_time = start.elapsed();

                    black_box(coordination_time)
                });
            },
        );
    }

    group.finish();
}

// =============================================================================
// Receipt Verification Latency Benchmarks
// =============================================================================

fn bench_receipt_verification_latency(c: &mut Criterion) {
    let mut group = c.benchmark_group("receipt_verification_latency");

    for chain_length in [5, 10, 25, 50].iter() {
        group.bench_with_input(
            BenchmarkId::new(
                "receipt_chain_verification",
                format!("{}_hops", chain_length),
            ),
            chain_length,
            |b, &chain_length| {
                b.to_async(tokio::runtime::Runtime::new().unwrap())
                    .iter(|| async {
                        let effects = LatencyTrackingEffects::new(
                            Duration::from_millis(20),
                            Duration::from_millis(5),
                            0.0,
                        );

                        let protocol =
                            ReceiptVerificationProtocol::new(ReceiptVerificationConfig::default());

                        // Create a chain of receipts to verify
                        let mut receipt_chain = Vec::new();
                        let mut current_hash = b"initial_message".to_vec();

                        for i in 0..chain_length {
                            let hop_data = format!("hop_{}_data", i).into_bytes();
                            current_hash = blake3::hash(&[&current_hash, &hop_data].concat())
                                .as_bytes()
                                .to_vec();
                            receipt_chain.push((DeviceId::new(), current_hash.clone()));
                        }

                        let start = Instant::now();
                        let _result = protocol.verify_receipt_chain(&effects, receipt_chain).await;
                        let verification_time = start.elapsed();

                        black_box(verification_time)
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
    protocol_latency_benches,
    bench_anti_entropy_round_trip_latency,
    bench_digest_exchange_latency,
    bench_journal_sync_end_to_end_latency,
    bench_journal_sync_message_latency,
    bench_latency_under_packet_loss,
    bench_jitter_impact_on_sync,
    bench_epoch_rotation_latency,
    bench_receipt_verification_latency,
);

criterion_main!(protocol_latency_benches);
