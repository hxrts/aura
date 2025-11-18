//! Simple working benchmarks for aura-sync protocols
//!
//! This provides a baseline set of benchmarks that work with the current API
//! and demonstrate the benchmarking framework capabilities.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::time::Instant;

use aura_core::{DeviceId, FactValue, Journal};
use aura_sync::core::MetricsCollector;
use aura_sync::protocols::{
    AntiEntropyConfig, AntiEntropyProtocol, EpochConfig, EpochRotationCoordinator,
    JournalSyncConfig, JournalSyncProtocol, OTAConfig, OTAProtocol, ReceiptVerificationConfig,
    ReceiptVerificationProtocol, SnapshotConfig, SnapshotProtocol,
};

// =============================================================================
// Test Data Generation
// =============================================================================

fn create_test_journal(op_count: usize) -> Journal {
    let mut journal = Journal::new();

    for i in 0..op_count {
        let key = format!("test_operation_{}", i);
        let value = FactValue::String(format!("test_data_{}", i));
        journal.facts.insert(key, value);
    }

    journal
}

// =============================================================================
// Protocol Creation Benchmarks
// =============================================================================

fn bench_protocol_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("protocol_creation");

    group.bench_function("anti_entropy_protocol", |b| {
        b.iter(|| {
            let config = AntiEntropyConfig::default();
            black_box(AntiEntropyProtocol::new(config))
        });
    });

    group.bench_function("journal_sync_protocol", |b| {
        b.iter(|| {
            let config = JournalSyncConfig::default();
            black_box(JournalSyncProtocol::new(config))
        });
    });

    group.bench_function("snapshot_protocol", |b| {
        b.iter(|| {
            let config = SnapshotConfig::default();
            black_box(SnapshotProtocol::new(config))
        });
    });

    group.bench_function("ota_protocol", |b| {
        b.iter(|| {
            let config = OTAConfig::default();
            black_box(OTAProtocol::new(config))
        });
    });

    group.bench_function("receipt_verification_protocol", |b| {
        b.iter(|| {
            let config = ReceiptVerificationConfig::default();
            black_box(ReceiptVerificationProtocol::new(config))
        });
    });

    group.bench_function("epoch_rotation_coordinator", |b| {
        b.iter(|| {
            let device_id = DeviceId::new();
            let config = EpochConfig::default();
            black_box(EpochRotationCoordinator::new(device_id, 0, config))
        });
    });

    group.finish();
}

// =============================================================================
// Digest Operations Benchmarks
// =============================================================================

fn bench_digest_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("digest_operations");

    for size in [100, 500, 1000, 2000].iter() {
        group.throughput(Throughput::Elements(*size as u64));
        group.bench_with_input(
            BenchmarkId::new("digest_computation", size),
            size,
            |b, &size| {
                let journal = create_test_journal(size);
                let protocol = AntiEntropyProtocol::new(AntiEntropyConfig::default());

                b.iter(|| {
                    let digest = black_box(protocol.compute_digest(&journal, &[]));
                    digest
                });
            },
        );
    }

    group.finish();
}

fn bench_digest_comparison(c: &mut Criterion) {
    let mut group = c.benchmark_group("digest_comparison");

    for size in [100, 500, 1000, 2000].iter() {
        group.throughput(Throughput::Elements(*size as u64));
        group.bench_with_input(
            BenchmarkId::new("digest_comparison", size),
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

// =============================================================================
// Journal State Management Benchmarks
// =============================================================================

fn bench_journal_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("journal_operations");

    for size in [100, 500, 1000, 5000].iter() {
        group.throughput(Throughput::Elements(*size as u64));
        group.bench_with_input(
            BenchmarkId::new("journal_creation", size),
            size,
            |b, &size| {
                b.iter(|| {
                    let journal = black_box(create_test_journal(size));
                    journal
                });
            },
        );
    }

    for peer_count in [5, 10, 25, 50].iter() {
        group.throughput(Throughput::Elements(*peer_count as u64));
        group.bench_with_input(
            BenchmarkId::new("peer_state_management", format!("{}_peers", peer_count)),
            peer_count,
            |b, &peer_count| {
                let mut protocol = JournalSyncProtocol::new(JournalSyncConfig::default());
                let peers: Vec<DeviceId> = (0..peer_count).map(|_| DeviceId::new()).collect();

                b.iter(|| {
                    for peer in &peers {
                        protocol.update_peer_state(
                            *peer,
                            black_box(aura_sync::protocols::SyncState::Syncing),
                        );
                        let _state = black_box(protocol.get_peer_state(peer));
                    }
                });
            },
        );
    }

    group.finish();
}

// =============================================================================
// Memory Usage Analysis
// =============================================================================

fn bench_memory_usage(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_usage");

    for journal_size in [1000, 5000, 10000].iter() {
        group.throughput(Throughput::Elements(*journal_size as u64));
        group.bench_with_input(
            BenchmarkId::new("journal_memory_overhead", format!("{}_ops", journal_size)),
            journal_size,
            |b, &journal_size| {
                b.iter(|| {
                    let journal = black_box(create_test_journal(journal_size));
                    let protocol =
                        black_box(AntiEntropyProtocol::new(AntiEntropyConfig::default()));
                    let digest = black_box(protocol.compute_digest(&journal, &[]));

                    // Simulate holding multiple journal states in memory
                    let mut journals = Vec::new();
                    for _i in 0..5 {
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
// Epoch Coordination Benchmarks
// =============================================================================

fn bench_epoch_coordination(c: &mut Criterion) {
    let mut group = c.benchmark_group("epoch_coordination");

    for participant_count in [3, 5, 10, 15].iter() {
        group.bench_with_input(
            BenchmarkId::new(
                "epoch_rotation_initiation",
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
                    let context_id = aura_core::ContextId::new("benchmark_context".to_string());

                    black_box(coordinator.initiate_rotation(participants, context_id))
                });
            },
        );
    }

    group.bench_function("epoch_confirmation_processing", |b| {
        let device_id = DeviceId::new();
        let config = EpochConfig::default();
        let mut coordinator = EpochRotationCoordinator::new(device_id, 0, config);

        let participant1 = DeviceId::new();
        let participant2 = DeviceId::new();
        let context_id = aura_core::ContextId::new("test_context".to_string());

        let rotation_id = coordinator
            .initiate_rotation(vec![participant1, participant2], context_id)
            .unwrap();

        let confirmation = aura_sync::protocols::EpochConfirmation {
            rotation_id: rotation_id.clone(),
            participant_id: participant1,
            current_epoch: 0,
            ready_for_epoch: 1,
            confirmation_timestamp: std::time::SystemTime::now(),
        };

        b.iter(|| black_box(coordinator.process_confirmation(confirmation.clone())));
    });

    group.finish();
}

// =============================================================================
// Configuration and Validation Benchmarks
// =============================================================================

fn bench_config_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("config_validation");

    group.bench_function("anti_entropy_config_creation", |b| {
        b.iter(|| black_box(AntiEntropyConfig::default()));
    });

    group.bench_function("journal_sync_config_creation", |b| {
        b.iter(|| black_box(JournalSyncConfig::default()));
    });

    group.bench_function("snapshot_config_creation", |b| {
        b.iter(|| black_box(SnapshotConfig::default()));
    });

    group.finish();
}

// =============================================================================
// Metrics Collection Benchmarks
// =============================================================================

fn bench_metrics_collection(c: &mut Criterion) {
    let mut group = c.benchmark_group("metrics_collection");

    group.bench_function("metrics_collector_creation", |b| {
        b.iter(|| black_box(MetricsCollector::new()));
    });

    group.bench_function("metrics_snapshot_export", |b| {
        let collector = MetricsCollector::new();

        // Add some sample metrics
        collector.record_sync_start("test_session", 1000000000);
        collector.record_sync_completion("test_session", 100, 1024, 1000000001);

        b.iter(|| black_box(collector.export_snapshot()));
    });

    group.bench_function("prometheus_export", |b| {
        let collector = MetricsCollector::new();

        // Add some sample metrics
        collector.record_sync_start("test_session", 1000000000);
        collector.record_sync_completion("test_session", 100, 1024, 1000000001);

        b.iter(|| black_box(collector.export_prometheus()));
    });

    group.finish();
}

// =============================================================================
// Performance Scaling Tests
// =============================================================================

fn bench_performance_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("performance_scaling");
    group.sample_size(30);

    group.bench_function("large_digest_computation_10k_ops", |b| {
        let large_journal = create_test_journal(10000);
        let protocol = AntiEntropyProtocol::new(AntiEntropyConfig::default());

        b.iter(|| black_box(protocol.compute_digest(&large_journal, &[])));
    });

    group.bench_function("concurrent_protocol_creation", |b| {
        b.iter(|| {
            let mut anti_entropy_protocols = Vec::new();
            let mut journal_sync_protocols = Vec::new();
            let mut snapshot_protocols = Vec::new();

            for _ in 0..100 {
                anti_entropy_protocols.push(AntiEntropyProtocol::new(AntiEntropyConfig::default()));
                journal_sync_protocols.push(JournalSyncProtocol::new(JournalSyncConfig::default()));
                snapshot_protocols.push(SnapshotProtocol::new(SnapshotConfig::default()));
            }

            black_box((
                anti_entropy_protocols,
                journal_sync_protocols,
                snapshot_protocols,
            ))
        });
    });

    group.finish();
}

// =============================================================================
// Criterion Configuration
// =============================================================================

criterion_group!(
    simple_benchmarks,
    bench_protocol_creation,
    bench_digest_operations,
    bench_digest_comparison,
    bench_journal_operations,
    bench_memory_usage,
    bench_epoch_coordination,
    bench_config_operations,
    bench_metrics_collection,
    bench_performance_scaling,
);

criterion_main!(simple_benchmarks);
