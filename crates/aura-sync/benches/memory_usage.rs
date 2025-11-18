//! Memory usage benchmarks for aura-sync protocols
//!
//! Measures memory consumption patterns during sync operations,
//! including peak memory usage and garbage collection impact.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use aura_core::effects::{JournalEffects, NetworkEffects, RandomEffects, TimeEffects};
use aura_core::{AuraError, DeviceId, FactValue, Journal};
use aura_sync::protocols::{
    AntiEntropyConfig, AntiEntropyProtocol, JournalSyncConfig, JournalSyncProtocol, OTAConfig,
    OTAProtocol, SnapshotConfig, SnapshotProtocol,
};

// =============================================================================
// Memory Tracking Utilities
// =============================================================================

struct MemoryTracker {
    peak_allocations: usize,
    current_allocations: usize,
    total_allocated: usize,
}

impl MemoryTracker {
    fn new() -> Self {
        Self {
            peak_allocations: 0,
            current_allocations: 0,
            total_allocated: 0,
        }
    }

    fn track_allocation(&mut self, size: usize) {
        self.current_allocations += size;
        self.total_allocated += size;
        self.peak_allocations = self.peak_allocations.max(self.current_allocations);
    }

    fn track_deallocation(&mut self, size: usize) {
        self.current_allocations = self.current_allocations.saturating_sub(size);
    }

    fn reset(&mut self) {
        *self = Self::new();
    }
}

// =============================================================================
// Mock Effects for Memory Testing
// =============================================================================

#[derive(Debug, Clone)]
pub struct MemoryTestEffects {
    journals: Arc<Mutex<HashMap<DeviceId, Journal>>>,
    network_buffers: Arc<Mutex<Vec<Vec<u8>>>>,
    memory_tracker: Arc<Mutex<MemoryTracker>>,
    current_time: Arc<Mutex<u64>>,
}

impl MemoryTestEffects {
    pub fn new() -> Self {
        Self {
            journals: Arc::new(Mutex::new(HashMap::new())),
            network_buffers: Arc::new(Mutex::new(Vec::new())),
            memory_tracker: Arc::new(Mutex::new(MemoryTracker::new())),
            current_time: Arc::new(Mutex::new(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
            )),
        }
    }

    pub fn track_memory(&self, size: usize) {
        self.memory_tracker.lock().unwrap().track_allocation(size);
    }

    pub fn untrack_memory(&self, size: usize) {
        self.memory_tracker.lock().unwrap().track_deallocation(size);
    }

    pub fn peak_memory(&self) -> usize {
        self.memory_tracker.lock().unwrap().peak_allocations
    }

    pub fn current_memory(&self) -> usize {
        self.memory_tracker.lock().unwrap().current_allocations
    }

    pub fn reset_memory_tracking(&self) {
        self.memory_tracker.lock().unwrap().reset();
    }

    pub fn with_journal(&self, device_id: DeviceId, journal: Journal) {
        let journal_size = estimate_journal_size(&journal);
        self.track_memory(journal_size);
        self.journals.lock().unwrap().insert(device_id, journal);
    }
}

impl JournalEffects for MemoryTestEffects {
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
        let journal_size = estimate_journal_size(&journal);
        self.track_memory(journal_size);
        self.journals.lock().unwrap().insert(device_id, journal);
        Ok(())
    }
}

impl NetworkEffects for MemoryTestEffects {
    async fn send_message(&self, _peer: DeviceId, data: Vec<u8>) -> Result<(), AuraError> {
        let data_size = data.len();
        self.track_memory(data_size);

        // Simulate network buffer storage
        self.network_buffers.lock().unwrap().push(data);

        // Simulate message cleanup after sending
        tokio::task::yield_now().await;
        self.untrack_memory(data_size);

        Ok(())
    }

    async fn receive_message(&self, _timeout: Duration) -> Result<(DeviceId, Vec<u8>), AuraError> {
        let mut buffers = self.network_buffers.lock().unwrap();
        if !buffers.is_empty() {
            let data = buffers.remove(0);
            Ok((DeviceId::new(), data))
        } else {
            Err(AuraError::Network("No messages available".to_string()))
        }
    }

    async fn broadcast_message(
        &self,
        peers: Vec<DeviceId>,
        data: Vec<u8>,
    ) -> Result<(), AuraError> {
        let total_size = data.len() * peers.len();
        self.track_memory(total_size);

        for peer in peers {
            self.send_message(peer, data.clone()).await?;
        }

        self.untrack_memory(total_size);
        Ok(())
    }
}

impl TimeEffects for MemoryTestEffects {
    async fn current_time(&self) -> u64 {
        *self.current_time.lock().unwrap()
    }

    async fn sleep(&self, duration: Duration) {
        tokio::time::sleep(duration).await;
    }
}

impl RandomEffects for MemoryTestEffects {
    async fn random_bytes(&self, length: usize) -> Vec<u8> {
        let data = (0..length).map(|i| (i % 256) as u8).collect();
        self.track_memory(length);
        data
    }

    async fn random_u64(&self) -> u64 {
        42 // Deterministic for benchmarks
    }
}

// =============================================================================
// Memory Estimation Utilities
// =============================================================================

fn estimate_journal_size(journal: &Journal) -> usize {
    let mut size = std::mem::size_of::<Journal>();

    // Estimate facts map size
    for (key, value) in &journal.facts {
        size += key.len();
        size += match value {
            FactValue::String(s) => s.len(),
            FactValue::Number(_) => 8,
            FactValue::Boolean(_) => 1,
        };
    }

    // Estimate caps size (simplified)
    size += journal.caps.len() * 64; // Rough estimate

    size
}

fn create_memory_intensive_journal(operation_count: usize, data_size_per_op: usize) -> Journal {
    let mut journal = Journal::new();

    for i in 0..operation_count {
        let key = format!("memory_test_operation_{}", i);
        let large_value = "x".repeat(data_size_per_op);
        let value = FactValue::String(format!("{}_{}", large_value, i));
        journal.facts.insert(key, value);
    }

    journal
}

// =============================================================================
// Protocol Memory Usage Benchmarks
// =============================================================================

fn bench_anti_entropy_memory_usage(c: &mut Criterion) {
    let mut group = c.benchmark_group("anti_entropy_memory");

    for ops_count in [100, 500, 1000, 2500].iter() {
        group.throughput(Throughput::Elements(*ops_count as u64));
        group.bench_with_input(
            BenchmarkId::new("full_protocol_memory", ops_count),
            ops_count,
            |b, &ops_count| {
                b.to_async(tokio::runtime::Runtime::new().unwrap())
                    .iter(|| async {
                        let effects = MemoryTestEffects::new();
                        effects.reset_memory_tracking();

                        let journal = create_memory_intensive_journal(ops_count, 200); // 200 bytes per op
                        let device_id = DeviceId::new();
                        let peer_id = DeviceId::new();

                        effects.with_journal(device_id, journal);

                        let protocol = AntiEntropyProtocol::new(AntiEntropyConfig::default());
                        let _result = protocol.execute(&effects, peer_id).await;

                        black_box((effects.peak_memory(), effects.current_memory()))
                    });
            },
        );
    }

    group.finish();
}

fn bench_journal_sync_memory_pressure(c: &mut Criterion) {
    let mut group = c.benchmark_group("journal_sync_memory_pressure");

    for concurrent_syncs in [1, 3, 5, 10].iter() {
        group.bench_with_input(
            BenchmarkId::new("concurrent_sync_memory", concurrent_syncs),
            concurrent_syncs,
            |b, &concurrent_syncs| {
                b.to_async(tokio::runtime::Runtime::new().unwrap())
                    .iter(|| async {
                        let effects = Arc::new(MemoryTestEffects::new());
                        effects.reset_memory_tracking();

                        let mut handles = Vec::new();

                        for i in 0..concurrent_syncs {
                            let effects_clone = effects.clone();
                            let handle = tokio::spawn(async move {
                                let journal = create_memory_intensive_journal(500, 150);
                                let device_id = DeviceId::new();
                                let peers = vec![DeviceId::new(), DeviceId::new()];

                                effects_clone.with_journal(device_id, journal);

                                let protocol =
                                    JournalSyncProtocol::new(JournalSyncConfig::default());
                                let _result =
                                    protocol.sync_with_peers(&*effects_clone, peers).await;
                            });
                            handles.push(handle);
                        }

                        for handle in handles {
                            let _ = handle.await;
                        }

                        black_box((effects.peak_memory(), effects.current_memory()))
                    });
            },
        );
    }

    group.finish();
}

fn bench_large_journal_memory_footprint(c: &mut Criterion) {
    let mut group = c.benchmark_group("large_journal_memory");

    for journal_size_mb in [1, 5, 10, 25].iter() {
        let ops_count = (journal_size_mb * 1024 * 1024) / 1000; // ~1KB per operation
        group.throughput(Throughput::Bytes(*journal_size_mb * 1024 * 1024));

        group.bench_with_input(
            BenchmarkId::new("large_journal_processing", format!("{}MB", journal_size_mb)),
            journal_size_mb,
            |b, &_journal_size_mb| {
                b.iter(|| {
                    let effects = MemoryTestEffects::new();
                    effects.reset_memory_tracking();

                    // Create progressively larger journals
                    let journal = create_memory_intensive_journal(ops_count, 1000);
                    let device_id = DeviceId::new();
                    effects.with_journal(device_id, journal.clone());

                    let protocol = AntiEntropyProtocol::new(AntiEntropyConfig::default());
                    let digest = protocol.create_digest(&journal);

                    black_box((digest, effects.peak_memory()))
                });
            },
        );
    }

    group.finish();
}

fn bench_snapshot_protocol_memory_cleanup(c: &mut Criterion) {
    let mut group = c.benchmark_group("snapshot_memory_cleanup");

    for snapshot_size in [1000, 5000, 10000].iter() {
        group.bench_with_input(
            BenchmarkId::new("snapshot_cleanup_cycle", snapshot_size),
            snapshot_size,
            |b, &snapshot_size| {
                b.to_async(tokio::runtime::Runtime::new().unwrap())
                    .iter(|| async {
                        let effects = MemoryTestEffects::new();
                        effects.reset_memory_tracking();

                        // Create a large journal that needs snapshotting
                        let journal = create_memory_intensive_journal(snapshot_size, 300);
                        let device_id = DeviceId::new();
                        effects.with_journal(device_id, journal);

                        let protocol = SnapshotProtocol::new(SnapshotConfig::default());

                        // Simulate snapshot creation, approval, and cleanup cycle
                        let peers = vec![DeviceId::new(), DeviceId::new(), DeviceId::new()];
                        let _result = protocol.coordinate_snapshot(&effects, peers).await;

                        black_box((effects.peak_memory(), effects.current_memory()))
                    });
            },
        );
    }

    group.finish();
}

fn bench_ota_update_memory_buffering(c: &mut Criterion) {
    let mut group = c.benchmark_group("ota_memory_buffering");

    for update_size_mb in [1, 5, 10, 20].iter() {
        group.throughput(Throughput::Bytes(*update_size_mb * 1024 * 1024));
        group.bench_with_input(
            BenchmarkId::new("ota_update_buffering", format!("{}MB", update_size_mb)),
            update_size_mb,
            |b, &update_size_mb| {
                b.to_async(tokio::runtime::Runtime::new().unwrap())
                    .iter(|| async {
                        let effects = MemoryTestEffects::new();
                        effects.reset_memory_tracking();

                        // Simulate large OTA update payload
                        let update_data = vec![0u8; update_size_mb * 1024 * 1024];
                        effects.track_memory(update_data.len());

                        let protocol = OTAProtocol::new(OTAConfig::default());
                        let peers = vec![DeviceId::new(), DeviceId::new()];

                        // Simulate OTA distribution process
                        let _result = protocol
                            .distribute_update(&effects, peers, update_data.clone())
                            .await;

                        effects.untrack_memory(update_data.len());

                        black_box((effects.peak_memory(), effects.current_memory()))
                    });
            },
        );
    }

    group.finish();
}

fn bench_memory_leak_detection(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_leak_detection");
    group.sample_size(50);

    group.bench_function("repeated_protocol_cycles", |b| {
        b.to_async(tokio::runtime::Runtime::new().unwrap())
            .iter(|| async {
                let effects = MemoryTestEffects::new();
                let mut memory_snapshots = Vec::new();

                // Run multiple protocol cycles and check for memory leaks
                for cycle in 0..10 {
                    effects.reset_memory_tracking();

                    let journal = create_memory_intensive_journal(200, 100);
                    let device_id = DeviceId::new();
                    let peer_id = DeviceId::new();

                    effects.with_journal(device_id, journal);

                    let protocol = AntiEntropyProtocol::new(AntiEntropyConfig::default());
                    let _result = protocol.execute(&effects, peer_id).await;

                    memory_snapshots.push((cycle, effects.current_memory()));
                }

                black_box(memory_snapshots)
            });
    });

    group.finish();
}

// =============================================================================
// Criterion Configuration
// =============================================================================

criterion_group!(
    memory_usage_benches,
    bench_anti_entropy_memory_usage,
    bench_journal_sync_memory_pressure,
    bench_large_journal_memory_footprint,
    bench_snapshot_protocol_memory_cleanup,
    bench_ota_update_memory_buffering,
    bench_memory_leak_detection,
);

criterion_main!(memory_usage_benches);
