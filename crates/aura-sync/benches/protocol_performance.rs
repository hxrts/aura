//! Comprehensive protocol performance benchmarks for aura-sync
//!
//! This is the primary benchmark suite that provides complete performance analysis
//! for all aura-sync protocols. It serves as the main entry point for performance
//! testing and integrates all specialized benchmark modules.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use aura_core::effects::{JournalEffects, NetworkEffects, RandomEffects, TimeEffects};
use aura_core::{AuraError, DeviceId, FactValue, Journal};
use aura_sync::core::{MetricsCollector, SyncMetricsSnapshot};
use aura_sync::protocols::{
    AntiEntropyConfig, AntiEntropyProtocol, EpochConfig, EpochRotationCoordinator,
    JournalSyncConfig, JournalSyncProtocol, OTAConfig, OTAProtocol, ReceiptVerificationConfig,
    ReceiptVerificationProtocol, SnapshotConfig, SnapshotProtocol,
};

// =============================================================================
// Integrated Performance Testing Framework
// =============================================================================

#[derive(Debug, Clone)]
pub struct IntegratedBenchmarkEffects {
    journals: Arc<Mutex<HashMap<DeviceId, Journal>>>,
    network_messages: Arc<Mutex<Vec<(DeviceId, Vec<u8>)>>>,
    metrics: Arc<MetricsCollector>,
    current_time: Arc<Mutex<u64>>,
    scenario_config: ScenarioConfig,
}

#[derive(Debug, Clone)]
pub struct ScenarioConfig {
    pub network_latency_ms: u64,
    pub packet_loss_rate: f32,
    pub processing_delay_us: u64,
    pub memory_pressure: bool,
}

impl ScenarioConfig {
    pub fn ideal() -> Self {
        Self {
            network_latency_ms: 0,
            packet_loss_rate: 0.0,
            processing_delay_us: 0,
            memory_pressure: false,
        }
    }

    pub fn realistic() -> Self {
        Self {
            network_latency_ms: 25,
            packet_loss_rate: 0.01,
            processing_delay_us: 100,
            memory_pressure: false,
        }
    }

    pub fn stressed() -> Self {
        Self {
            network_latency_ms: 100,
            packet_loss_rate: 0.05,
            processing_delay_us: 500,
            memory_pressure: true,
        }
    }
}

impl IntegratedBenchmarkEffects {
    pub fn new(scenario: ScenarioConfig) -> Self {
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
            scenario_config: scenario,
        }
    }

    pub fn with_journal(&self, device_id: DeviceId, journal: Journal) {
        self.journals.lock().unwrap().insert(device_id, journal);
    }

    pub fn get_metrics_snapshot(&self) -> SyncMetricsSnapshot {
        self.metrics.export_snapshot()
    }
}

impl JournalEffects for IntegratedBenchmarkEffects {
    async fn get_journal(&self) -> Result<Journal, AuraError> {
        if self.scenario_config.processing_delay_us > 0 {
            tokio::time::sleep(Duration::from_micros(
                self.scenario_config.processing_delay_us,
            ))
            .await;
        }

        let device_id = DeviceId::new();
        self.journals
            .lock()
            .unwrap()
            .get(&device_id)
            .cloned()
            .ok_or_else(|| AuraError::Storage("No journal found".to_string()))
    }

    async fn update_journal(&self, journal: Journal) -> Result<(), AuraError> {
        if self.scenario_config.processing_delay_us > 0 {
            tokio::time::sleep(Duration::from_micros(
                self.scenario_config.processing_delay_us,
            ))
            .await;
        }

        let device_id = DeviceId::new();
        self.journals.lock().unwrap().insert(device_id, journal);
        Ok(())
    }
}

impl NetworkEffects for IntegratedBenchmarkEffects {
    async fn send_message(&self, peer: DeviceId, data: Vec<u8>) -> Result<(), AuraError> {
        // Simulate packet loss
        if rand::random::<f32>() < self.scenario_config.packet_loss_rate {
            return Err(AuraError::Network("Packet dropped".to_string()));
        }

        // Simulate network latency
        if self.scenario_config.network_latency_ms > 0 {
            tokio::time::sleep(Duration::from_millis(
                self.scenario_config.network_latency_ms,
            ))
            .await;
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

impl TimeEffects for IntegratedBenchmarkEffects {
    async fn current_time(&self) -> u64 {
        *self.current_time.lock().unwrap()
    }

    async fn sleep(&self, duration: Duration) {
        tokio::time::sleep(duration).await;
    }
}

impl RandomEffects for IntegratedBenchmarkEffects {
    async fn random_bytes(&self, length: usize) -> Vec<u8> {
        (0..length).map(|i| (i % 256) as u8).collect()
    }

    async fn random_u64(&self) -> u64 {
        rand::random()
    }
}

// =============================================================================
// Test Data Generation
// =============================================================================

fn create_test_journal(op_count: usize, payload_size: usize) -> Journal {
    let mut journal = Journal::new();

    for i in 0..op_count {
        let key = format!("integrated_test_operation_{}", i);
        let payload = "x".repeat(payload_size);
        let value = FactValue::String(format!("{}_{}", payload, i));
        journal.facts.insert(key, value);
    }

    journal
}

// =============================================================================
// Integrated Protocol Performance Benchmarks
// =============================================================================

fn bench_protocol_performance_scenarios(c: &mut Criterion) {
    let mut group = c.benchmark_group("integrated_protocol_scenarios");

    let scenarios = [
        ("ideal", ScenarioConfig::ideal()),
        ("realistic", ScenarioConfig::realistic()),
        ("stressed", ScenarioConfig::stressed()),
    ];

    for (scenario_name, scenario_config) in scenarios.iter() {
        group.bench_with_input(
            BenchmarkId::new("anti_entropy_full_cycle", scenario_name),
            scenario_name,
            |b, &_scenario_name| {
                b.to_async(tokio::runtime::Runtime::new().unwrap())
                    .iter(|| async {
                        let effects = IntegratedBenchmarkEffects::new(scenario_config.clone());

                        let journal1 = create_test_journal(200, 150);
                        let journal2 = create_test_journal(180, 150); // Slightly different
                        let device_id = DeviceId::new();
                        let peer_id = DeviceId::new();

                        effects.with_journal(device_id, journal1);
                        effects.with_journal(peer_id, journal2);

                        let protocol = AntiEntropyProtocol::new(AntiEntropyConfig::default());

                        let _result = black_box(protocol.execute(&effects, peer_id).await);
                        let metrics = effects.get_metrics_snapshot();

                        black_box(metrics)
                    });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("journal_sync_multi_peer", scenario_name),
            scenario_name,
            |b, &_scenario_name| {
                b.to_async(tokio::runtime::Runtime::new().unwrap())
                    .iter(|| async {
                        let effects = IntegratedBenchmarkEffects::new(scenario_config.clone());

                        let journal = create_test_journal(150, 100);
                        let device_id = DeviceId::new();
                        let peers = vec![DeviceId::new(), DeviceId::new(), DeviceId::new()];

                        effects.with_journal(device_id, journal.clone());
                        for peer in &peers {
                            effects.with_journal(*peer, journal.clone());
                        }

                        let protocol = JournalSyncProtocol::new(JournalSyncConfig::default());

                        let _result = black_box(protocol.sync_with_peers(&effects, peers).await);
                        let metrics = effects.get_metrics_snapshot();

                        black_box(metrics)
                    });
            },
        );
    }

    group.finish();
}

fn bench_protocol_creation_overhead(c: &mut Criterion) {
    let mut group = c.benchmark_group("protocol_creation_overhead");

    group.bench_function("anti_entropy_creation", |b| {
        b.iter(|| {
            let config = AntiEntropyConfig::default();
            black_box(AntiEntropyProtocol::new(config))
        });
    });

    group.bench_function("journal_sync_creation", |b| {
        b.iter(|| {
            let config = JournalSyncConfig::default();
            black_box(JournalSyncProtocol::new(config))
        });
    });

    group.bench_function("snapshot_protocol_creation", |b| {
        b.iter(|| {
            let config = SnapshotConfig::default();
            black_box(SnapshotProtocol::new(config))
        });
    });

    group.bench_function("ota_protocol_creation", |b| {
        b.iter(|| {
            let config = OTAConfig::default();
            black_box(OTAProtocol::new(config))
        });
    });

    group.bench_function("receipt_verification_creation", |b| {
        b.iter(|| {
            let config = ReceiptVerificationConfig::default();
            black_box(ReceiptVerificationProtocol::new(config))
        });
    });

    group.finish();
}

fn bench_end_to_end_protocol_workflows(c: &mut Criterion) {
    let mut group = c.benchmark_group("end_to_end_workflows");
    group.sample_size(30); // Reduce for expensive end-to-end tests

    group.bench_function("snapshot_coordination_workflow", |b| {
        b.to_async(tokio::runtime::Runtime::new().unwrap())
            .iter(|| async {
                let effects = IntegratedBenchmarkEffects::new(ScenarioConfig::realistic());

                let journal = create_test_journal(500, 200);
                let device_id = DeviceId::new();
                let participants = vec![DeviceId::new(), DeviceId::new(), DeviceId::new()];

                effects.with_journal(device_id, journal.clone());
                for participant in &participants {
                    effects.with_journal(*participant, journal.clone());
                }

                let protocol = SnapshotProtocol::new(SnapshotConfig::default());

                let _result = black_box(protocol.coordinate_snapshot(&effects, participants).await);
                let metrics = effects.get_metrics_snapshot();

                black_box(metrics)
            });
    });

    group.bench_function("ota_distribution_workflow", |b| {
        b.to_async(tokio::runtime::Runtime::new().unwrap())
            .iter(|| async {
                let effects = IntegratedBenchmarkEffects::new(ScenarioConfig::realistic());

                let update_data = vec![0u8; 256 * 1024]; // 256KB update
                let nodes = vec![
                    DeviceId::new(),
                    DeviceId::new(),
                    DeviceId::new(),
                    DeviceId::new(),
                ];

                for node in &nodes {
                    effects.with_journal(*node, Journal::new());
                }

                let protocol = OTAProtocol::new(OTAConfig::default());

                let _result = black_box(
                    protocol
                        .distribute_update(&effects, nodes, update_data)
                        .await,
                );
                let metrics = effects.get_metrics_snapshot();

                black_box(metrics)
            });
    });

    group.finish();
}

fn bench_epoch_coordination_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("epoch_coordination_operations");

    group.bench_function("epoch_rotation_initiation", |b| {
        b.iter(|| {
            let device_id = DeviceId::new();
            let config = EpochConfig::default();
            let mut coordinator = EpochRotationCoordinator::new(device_id, 0, config);

            let participants = vec![DeviceId::new(), DeviceId::new()];
            let context_id = aura_core::ContextId::new();

            black_box(coordinator.initiate_rotation(participants, context_id))
        });
    });

    group.bench_function("epoch_confirmation_processing", |b| {
        let device_id = DeviceId::new();
        let config = EpochConfig::default();
        let mut coordinator = EpochRotationCoordinator::new(device_id, 0, config);

        let participant1 = DeviceId::new();
        let participant2 = DeviceId::new();
        let context_id = aura_core::ContextId::new();

        let rotation_id = coordinator
            .initiate_rotation(vec![participant1, participant2], context_id)
            .unwrap();

        let confirmation = aura_sync::protocols::EpochConfirmation {
            rotation_id: rotation_id.clone(),
            participant_id: participant1,
            current_epoch: 0,
            ready_for_epoch: 1,
            confirmation_timestamp: SystemTime::now(),
        };

        b.iter(|| black_box(coordinator.process_confirmation(confirmation.clone())));
    });

    group.finish();
}

// =============================================================================
// Configuration and Validation Benchmarks
// =============================================================================

fn bench_config_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("config_operations");

    group.bench_function("anti_entropy_config_validation", |b| {
        let mut config = AntiEntropyConfig::default();
        b.iter(|| black_box(config.validate()));
    });

    group.bench_function("journal_sync_config_validation", |b| {
        let mut config = JournalSyncConfig::default();
        b.iter(|| black_box(config.validate()));
    });

    group.bench_function("snapshot_config_validation", |b| {
        let mut config = SnapshotConfig::default();
        b.iter(|| black_box(config.validate()));
    });

    group.finish();
}

// =============================================================================
// Criterion Configuration
// =============================================================================

criterion_group!(
    protocol_performance_benches,
    bench_protocol_performance_scenarios,
    bench_protocol_creation_overhead,
    bench_end_to_end_protocol_workflows,
    bench_epoch_coordination_operations,
    bench_config_operations,
);

criterion_main!(protocol_performance_benches);
