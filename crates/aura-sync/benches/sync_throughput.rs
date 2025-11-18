//! Sync message throughput benchmarks
//!
//! Measures the throughput of sync messages across different protocols
//! under various network conditions and data sizes.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use aura_core::effects::{JournalEffects, NetworkEffects, RandomEffects, TimeEffects};
use aura_core::{AuraError, DeviceId, FactValue, Journal};
use aura_sync::core::MetricsCollector;
use aura_sync::protocols::{
    AntiEntropyConfig, AntiEntropyProtocol, JournalDigest, JournalSyncConfig, JournalSyncProtocol,
    SyncMessage, SyncState,
};
use aura_testkit::foundation::MockEffectSystem;

// =============================================================================
// Mock Effect System for Benchmarks
// =============================================================================

#[derive(Debug, Clone)]
pub struct BenchmarkEffects {
    pub journals: Arc<Mutex<HashMap<DeviceId, Journal>>>,
    pub network_messages: Arc<Mutex<Vec<(DeviceId, Vec<u8>)>>>,
    pub metrics: Arc<MetricsCollector>,
    pub current_time: Arc<Mutex<u64>>,
    pub latency_ms: u64,
}

impl BenchmarkEffects {
    pub fn new(latency_ms: u64) -> Self {
        Self {
            journals: Arc::new(Mutex::new(HashMap::new())),
            network_messages: Arc::new(Mutex::new(Vec::new())),
            metrics: Arc::new(MetricsCollector::new()),
            current_time: Arc::new(Mutex::new(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
            )),
            latency_ms,
        }
    }

    pub fn with_journal(&self, device_id: DeviceId, journal: Journal) {
        self.journals.lock().unwrap().insert(device_id, journal);
    }

    pub fn message_count(&self) -> usize {
        self.network_messages.lock().unwrap().len()
    }

    pub fn clear_messages(&self) {
        self.network_messages.lock().unwrap().clear();
    }
}

impl JournalEffects for BenchmarkEffects {
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

impl NetworkEffects for BenchmarkEffects {
    async fn send_message(&self, peer: DeviceId, data: Vec<u8>) -> Result<(), AuraError> {
        // Simulate network latency
        if self.latency_ms > 0 {
            tokio::time::sleep(Duration::from_millis(self.latency_ms)).await;
        }

        self.network_messages.lock().unwrap().push((peer, data));
        Ok(())
    }

    async fn receive_message(&self, _timeout: Duration) -> Result<(DeviceId, Vec<u8>), AuraError> {
        let mut messages = self.network_messages.lock().unwrap();
        if !messages.is_empty() {
            Ok(messages.remove(0))
        } else {
            Err(AuraError::Network("No messages available".to_string()))
        }
    }

    async fn broadcast_message(
        &self,
        peers: Vec<DeviceId>,
        data: Vec<u8>,
    ) -> Result<(), AuraError> {
        for peer in peers {
            self.send_message(peer, data.clone()).await?;
        }
        Ok(())
    }
}

impl TimeEffects for BenchmarkEffects {
    async fn current_time(&self) -> u64 {
        *self.current_time.lock().unwrap()
    }

    async fn sleep(&self, duration: Duration) {
        tokio::time::sleep(duration).await;
    }
}

impl RandomEffects for BenchmarkEffects {
    async fn random_bytes(&self, length: usize) -> Vec<u8> {
        (0..length).map(|i| (i % 256) as u8).collect()
    }

    async fn random_u64(&self) -> u64 {
        42 // Deterministic for benchmarks
    }
}

// =============================================================================
// Test Data Generation
// =============================================================================

fn create_test_journal(operation_count: usize) -> Journal {
    let mut journal = Journal::new();

    for i in 0..operation_count {
        let key = format!("test_operation_{}", i);
        let value = FactValue::String(format!("operation_data_{}", i));
        journal.facts.insert(key, value);
    }

    journal
}

fn create_large_sync_message(ops_count: usize) -> SyncMessage {
    let mut operations = Vec::new();

    for i in 0..ops_count {
        // Simulate realistic operation size (~200 bytes each)
        let operation_data = format!(
            "operation_{}_with_substantial_data_payload_that_represents_realistic_sync_content_{}",
            i,
            "x".repeat(100)
        );
        operations.push(operation_data.into_bytes());
    }

    SyncMessage::OperationsResponse {
        operations: operations.into_iter().map(|data| data.into()).collect(),
        has_more: false,
    }
}

// =============================================================================
// Anti-Entropy Protocol Benchmarks
// =============================================================================

fn bench_anti_entropy_digest_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("anti_entropy_digest");

    for size in [10, 100, 1000, 10000].iter() {
        group.throughput(Throughput::Elements(*size as u64));
        group.bench_with_input(BenchmarkId::new("create_digest", size), size, |b, &size| {
            let journal = create_test_journal(size);
            let effects = BenchmarkEffects::new(0);
            effects.with_journal(DeviceId::new(), journal.clone());

            b.iter(|| {
                let protocol = AntiEntropyProtocol::new(AntiEntropyConfig::default());
                let digest = black_box(protocol.compute_digest(&journal, &[]));
                digest
            });
        });
    }

    group.finish();
}

fn bench_anti_entropy_digest_comparison(c: &mut Criterion) {
    let mut group = c.benchmark_group("anti_entropy_comparison");

    for size in [100, 1000, 10000].iter() {
        group.throughput(Throughput::Elements(*size as u64));
        group.bench_with_input(
            BenchmarkId::new("compare_digests", size),
            size,
            |b, &size| {
                let journal1 = create_test_journal(size);
                let mut journal2 = create_test_journal(size);

                // Make journals slightly different
                journal2.facts.insert(
                    "extra_key".to_string(),
                    FactValue::String("extra_value".to_string()),
                );

                let protocol = AntiEntropyProtocol::new(AntiEntropyConfig::default());
                let digest1 = protocol.compute_digest(&journal1, &[]).unwrap();
                let digest2 = protocol.compute_digest(&journal2, &[]).unwrap();

                b.iter(|| {
                    let status = black_box(AntiEntropyProtocol::compare(&digest1, &digest2));
                    status
                });
            },
        );
    }

    group.finish();
}

fn bench_anti_entropy_full_sync(c: &mut Criterion) {
    let mut group = c.benchmark_group("anti_entropy_full_sync");
    group.sample_size(50); // Reduce sample size for expensive operations

    for size in [100, 500, 1000].iter() {
        group.throughput(Throughput::Elements(*size as u64));
        group.bench_with_input(
            BenchmarkId::new("full_sync_protocol", size),
            size,
            |b, &size| {
                b.iter_batched(
                    || {
                        let effects = BenchmarkEffects::new(1); // 1ms latency
                        let journal1 = create_test_journal(size);
                        let journal2 = create_test_journal(size / 2); // Partial sync needed

                        let device1 = DeviceId::new();
                        let device2 = DeviceId::new();

                        effects.with_journal(device1, journal1);
                        effects.with_journal(device2, journal2);

                        (effects, device2)
                    },
                    |(effects, device2)| {
                        let rt = tokio::runtime::Runtime::new().unwrap();
                        rt.block_on(async {
                            let protocol = AntiEntropyProtocol::new(AntiEntropyConfig::default());
                            let _result = black_box(
                                protocol
                                    .execute(&effects, device2)
                                    .await
                                    .unwrap_or_default(),
                            );
                        })
                    },
                    criterion::BatchSize::SmallInput,
                );
            },
        );
    }

    group.finish();
}

// =============================================================================
// Journal Sync Protocol Benchmarks
// =============================================================================

fn bench_journal_sync_message_processing(c: &mut Criterion) {
    let mut group = c.benchmark_group("journal_sync_messages");

    for ops_count in [10, 50, 100, 500].iter() {
        group.throughput(Throughput::Elements(*ops_count as u64));
        group.bench_with_input(
            BenchmarkId::new("process_operations_response", ops_count),
            ops_count,
            |b, &ops_count| {
                let message = create_large_sync_message(ops_count);
                let protocol = JournalSyncProtocol::new(JournalSyncConfig::default());

                b.to_async(tokio::runtime::Runtime::new().unwrap())
                    .iter(|| async {
                        let effects = BenchmarkEffects::new(0);
                        let peer = DeviceId::new();

                        let _result = black_box(
                            protocol
                                .handle_sync_message(&effects, peer, message.clone())
                                .await,
                        );
                    });
            },
        );
    }

    group.finish();
}

fn bench_journal_sync_state_management(c: &mut Criterion) {
    let mut group = c.benchmark_group("journal_sync_state");

    for peer_count in [5, 10, 25, 50, 100].iter() {
        group.throughput(Throughput::Elements(*peer_count as u64));
        group.bench_with_input(
            BenchmarkId::new("manage_peer_states", peer_count),
            peer_count,
            |b, &peer_count| {
                let mut protocol = JournalSyncProtocol::new(JournalSyncConfig::default());
                let peers: Vec<DeviceId> = (0..*peer_count).map(|_| DeviceId::new()).collect();

                b.iter(|| {
                    for peer in &peers {
                        protocol.update_peer_state(*peer, black_box(SyncState::Syncing));
                        let _state = black_box(protocol.get_peer_state(peer));
                    }
                });
            },
        );
    }

    group.finish();
}

// =============================================================================
// Network Latency Impact Benchmarks
// =============================================================================

fn bench_sync_with_network_latency(c: &mut Criterion) {
    let mut group = c.benchmark_group("network_latency_impact");
    group.sample_size(20);

    for latency_ms in [0, 10, 50, 100, 250].iter() {
        group.bench_with_input(
            BenchmarkId::new("anti_entropy_with_latency", latency_ms),
            latency_ms,
            |b, &latency_ms| {
                b.iter_batched(
                    || {
                        let effects = BenchmarkEffects::new(latency_ms);
                        let journal = create_test_journal(100);

                        let device_id = DeviceId::new();
                        let peer_id = DeviceId::new();

                        effects.with_journal(device_id, journal);

                        (effects, peer_id)
                    },
                    |(effects, peer_id)| {
                        let rt = tokio::runtime::Runtime::new().unwrap();
                        rt.block_on(async {
                            let protocol = AntiEntropyProtocol::new(AntiEntropyConfig::default());

                            // Simulate digest exchange
                            let dummy_data = b"digest_data".to_vec();
                            let _result =
                                black_box(effects.send_message(peer_id, dummy_data).await);
                        })
                    },
                    criterion::BatchSize::SmallInput,
                );
            },
        );
    }

    group.finish();
}

// =============================================================================
// Memory Usage Benchmarks
// =============================================================================

fn bench_memory_usage_during_sync(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_usage");
    group.sample_size(30);

    for journal_size in [1000, 5000, 10000].iter() {
        group.throughput(Throughput::Elements(*journal_size as u64));
        group.bench_with_input(
            BenchmarkId::new("journal_memory_overhead", journal_size),
            journal_size,
            |b, &journal_size| {
                b.iter(|| {
                    let journal = black_box(create_test_journal(journal_size));
                    let protocol =
                        black_box(AntiEntropyProtocol::new(AntiEntropyConfig::default()));
                    let digest = black_box(protocol.create_digest(&journal));

                    // Simulate holding multiple journal states in memory
                    let mut journals = Vec::new();
                    for i in 0..10 {
                        journals.push(create_test_journal(journal_size / 10));
                    }

                    black_box((digest, journals))
                });
            },
        );
    }

    group.finish();
}

// =============================================================================
// Concurrent Operations Benchmarks
// =============================================================================

fn bench_concurrent_sync_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("concurrent_operations");
    group.sample_size(20);

    for concurrent_syncs in [2, 5, 10].iter() {
        group.bench_with_input(
            BenchmarkId::new("concurrent_anti_entropy", concurrent_syncs),
            concurrent_syncs,
            |b, &concurrent_syncs| {
                b.to_async(tokio::runtime::Runtime::new().unwrap())
                    .iter(|| async {
                        let effects = Arc::new(BenchmarkEffects::new(5));

                        let mut handles = Vec::new();

                        for i in 0..concurrent_syncs {
                            let effects_clone = effects.clone();
                            let handle = tokio::spawn(async move {
                                let journal = create_test_journal(100);
                                let device_id = DeviceId::new();
                                let peer_id = DeviceId::new();

                                effects_clone.with_journal(device_id, journal);

                                let protocol =
                                    AntiEntropyProtocol::new(AntiEntropyConfig::default());
                                let _result = protocol.execute(&*effects_clone, peer_id).await;
                            });
                            handles.push(handle);
                        }

                        for handle in handles {
                            let _ = handle.await;
                        }

                        black_box(())
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
    sync_throughput_benches,
    bench_anti_entropy_digest_creation,
    bench_anti_entropy_digest_comparison,
    bench_anti_entropy_full_sync,
    bench_journal_sync_message_processing,
    bench_journal_sync_state_management,
    bench_sync_with_network_latency,
    bench_memory_usage_during_sync,
    bench_concurrent_sync_operations,
);

criterion_main!(sync_throughput_benches);
