//! Performance analysis and reporting framework for aura-sync
//!
//! This benchmark provides comprehensive performance analysis tools that
//! collect baseline metrics, generate reports, and provide actionable insights.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use aura_core::effects::{JournalEffects, NetworkEffects, RandomEffects, TimeEffects};
use aura_core::{AuraError, DeviceId, FactValue, Journal};
use aura_sync::core::{MetricsCollector, SyncMetricsSnapshot};
use aura_sync::protocols::{
    AntiEntropyConfig, AntiEntropyProtocol, JournalSyncConfig, JournalSyncProtocol, OTAConfig,
    OTAProtocol, SnapshotConfig, SnapshotProtocol,
};

// =============================================================================
// Performance Tracking Framework
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceBaseline {
    pub protocol_name: String,
    pub test_scenario: String,
    pub operations_per_second: f64,
    pub average_latency_ms: f64,
    pub p95_latency_ms: f64,
    pub p99_latency_ms: f64,
    pub memory_usage_mb: f64,
    pub network_bandwidth_mbps: f64,
    pub success_rate: f64,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolComparison {
    pub baseline: PerformanceBaseline,
    pub current: PerformanceBaseline,
    pub throughput_change_percent: f64,
    pub latency_change_percent: f64,
    pub memory_change_percent: f64,
    pub recommendation: String,
}

#[derive(Debug, Clone)]
pub struct PerformanceTracker {
    baselines: Arc<Mutex<HashMap<String, PerformanceBaseline>>>,
    current_metrics: Arc<MetricsCollector>,
    latency_samples: Arc<Mutex<Vec<Duration>>>,
    memory_samples: Arc<Mutex<Vec<usize>>>,
}

impl PerformanceTracker {
    pub fn new() -> Self {
        Self {
            baselines: Arc::new(Mutex::new(HashMap::new())),
            current_metrics: Arc::new(MetricsCollector::new()),
            latency_samples: Arc::new(Mutex::new(Vec::new())),
            memory_samples: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn record_latency(&self, latency: Duration) {
        self.latency_samples.lock().unwrap().push(latency);
    }

    pub fn record_memory_usage(&self, memory_mb: usize) {
        self.memory_samples.lock().unwrap().push(memory_mb);
    }

    pub fn create_baseline(&self, protocol_name: &str, scenario: &str) -> PerformanceBaseline {
        let latencies = self.latency_samples.lock().unwrap();
        let memory_samples = self.memory_samples.lock().unwrap();
        let metrics_snapshot = self.current_metrics.export_snapshot();

        let (avg_latency, p95_latency, p99_latency) = if !latencies.is_empty() {
            let mut sorted_latencies = latencies.clone();
            sorted_latencies.sort();

            let avg = sorted_latencies.iter().sum::<Duration>().as_millis() as f64
                / latencies.len() as f64;
            let p95_index = (latencies.len() as f32 * 0.95) as usize;
            let p99_index = (latencies.len() as f32 * 0.99) as usize;

            let p95 = sorted_latencies
                .get(p95_index)
                .unwrap_or(&Duration::ZERO)
                .as_millis() as f64;
            let p99 = sorted_latencies
                .get(p99_index)
                .unwrap_or(&Duration::ZERO)
                .as_millis() as f64;

            (avg, p95, p99)
        } else {
            (0.0, 0.0, 0.0)
        };

        let avg_memory = if !memory_samples.is_empty() {
            memory_samples.iter().sum::<usize>() as f64 / memory_samples.len() as f64
        } else {
            0.0
        };

        let ops_per_second = if avg_latency > 0.0 {
            1000.0 / avg_latency
        } else {
            0.0
        };

        let success_rate = if metrics_snapshot.operational.sync_sessions_total > 0 {
            (metrics_snapshot.operational.sync_sessions_completed_total as f64
                / metrics_snapshot.operational.sync_sessions_total as f64)
                * 100.0
        } else {
            100.0
        };

        PerformanceBaseline {
            protocol_name: protocol_name.to_string(),
            test_scenario: scenario.to_string(),
            operations_per_second: ops_per_second,
            average_latency_ms: avg_latency,
            p95_latency_ms: p95_latency,
            p99_latency_ms: p99_latency,
            memory_usage_mb: avg_memory,
            network_bandwidth_mbps: metrics_snapshot.resources.network_bandwidth_bps as f64
                / (1024.0 * 1024.0),
            success_rate,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        }
    }

    pub fn compare_with_baseline(
        &self,
        current: PerformanceBaseline,
        baseline: PerformanceBaseline,
    ) -> ProtocolComparison {
        let throughput_change = if baseline.operations_per_second > 0.0 {
            ((current.operations_per_second - baseline.operations_per_second)
                / baseline.operations_per_second)
                * 100.0
        } else {
            0.0
        };

        let latency_change = if baseline.average_latency_ms > 0.0 {
            ((current.average_latency_ms - baseline.average_latency_ms)
                / baseline.average_latency_ms)
                * 100.0
        } else {
            0.0
        };

        let memory_change = if baseline.memory_usage_mb > 0.0 {
            ((current.memory_usage_mb - baseline.memory_usage_mb) / baseline.memory_usage_mb)
                * 100.0
        } else {
            0.0
        };

        let recommendation =
            if throughput_change < -10.0 || latency_change > 20.0 || memory_change > 30.0 {
                "Performance regression detected - investigate recent changes".to_string()
            } else if throughput_change > 10.0 && latency_change < -5.0 {
                "Performance improvement detected - consider this baseline".to_string()
            } else {
                "Performance within acceptable range".to_string()
            };

        ProtocolComparison {
            baseline,
            current,
            throughput_change_percent: throughput_change,
            latency_change_percent: latency_change,
            memory_change_percent: memory_change,
            recommendation,
        }
    }

    pub fn clear_samples(&self) {
        self.latency_samples.lock().unwrap().clear();
        self.memory_samples.lock().unwrap().clear();
    }

    pub fn export_report(&self, comparisons: &[ProtocolComparison]) -> String {
        let mut report = String::new();

        report.push_str("# Aura-Sync Performance Report\n\n");
        report.push_str(&format!(
            "Generated: {}\n\n",
            chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
        ));

        report.push_str("## Executive Summary\n\n");

        let avg_throughput_change: f64 = comparisons
            .iter()
            .map(|c| c.throughput_change_percent)
            .sum::<f64>()
            / comparisons.len() as f64;

        let avg_latency_change: f64 = comparisons
            .iter()
            .map(|c| c.latency_change_percent)
            .sum::<f64>()
            / comparisons.len() as f64;

        report.push_str(&format!(
            "- Average throughput change: {:+.2}%\n",
            avg_throughput_change
        ));
        report.push_str(&format!(
            "- Average latency change: {:+.2}%\n",
            avg_latency_change
        ));
        report.push_str(&format!("- Protocols tested: {}\n\n", comparisons.len()));

        report.push_str("## Protocol Performance Details\n\n");

        for comparison in comparisons {
            report.push_str(&format!(
                "### {} - {}\n\n",
                comparison.current.protocol_name, comparison.current.test_scenario
            ));
            report.push_str("| Metric | Baseline | Current | Change |\n");
            report.push_str("|--------|----------|---------|--------|\n");
            report.push_str(&format!(
                "| Throughput (ops/sec) | {:.2} | {:.2} | {:+.2}% |\n",
                comparison.baseline.operations_per_second,
                comparison.current.operations_per_second,
                comparison.throughput_change_percent
            ));
            report.push_str(&format!(
                "| Avg Latency (ms) | {:.2} | {:.2} | {:+.2}% |\n",
                comparison.baseline.average_latency_ms,
                comparison.current.average_latency_ms,
                comparison.latency_change_percent
            ));
            report.push_str(&format!(
                "| P95 Latency (ms) | {:.2} | {:.2} | - |\n",
                comparison.baseline.p95_latency_ms, comparison.current.p95_latency_ms
            ));
            report.push_str(&format!(
                "| Memory Usage (MB) | {:.2} | {:.2} | {:+.2}% |\n",
                comparison.baseline.memory_usage_mb,
                comparison.current.memory_usage_mb,
                comparison.memory_change_percent
            ));
            report.push_str(&format!(
                "| Success Rate (%) | {:.2} | {:.2} | - |\n\n",
                comparison.baseline.success_rate, comparison.current.success_rate
            ));
            report.push_str(&format!(
                "**Recommendation:** {}\n\n",
                comparison.recommendation
            ));
        }

        report.push_str("## Performance Recommendations\n\n");

        let regressions: Vec<_> = comparisons
            .iter()
            .filter(|c| c.throughput_change_percent < -10.0 || c.latency_change_percent > 20.0)
            .collect();

        if !regressions.is_empty() {
            report.push_str("### 🚨 Performance Regressions Detected\n\n");
            for regression in regressions {
                report.push_str(&format!(
                    "- **{}**: {} (Throughput: {:+.1}%, Latency: {:+.1}%)\n",
                    regression.current.protocol_name,
                    regression.current.test_scenario,
                    regression.throughput_change_percent,
                    regression.latency_change_percent
                ));
            }
            report.push_str("\n");
        }

        let improvements: Vec<_> = comparisons
            .iter()
            .filter(|c| c.throughput_change_percent > 10.0 && c.latency_change_percent < -5.0)
            .collect();

        if !improvements.is_empty() {
            report.push_str("### ✅ Performance Improvements\n\n");
            for improvement in improvements {
                report.push_str(&format!(
                    "- **{}**: {} (Throughput: +{:.1}%, Latency: {:.1}%)\n",
                    improvement.current.protocol_name,
                    improvement.current.test_scenario,
                    improvement.throughput_change_percent,
                    improvement.latency_change_percent
                ));
            }
            report.push_str("\n");
        }

        report.push_str("### General Optimization Opportunities\n\n");
        report.push_str("1. **Network Optimization**: Consider message batching for protocols with high message volume\n");
        report.push_str(
            "2. **Memory Management**: Implement streaming for large journal operations\n",
        );
        report.push_str("3. **Concurrency**: Evaluate opportunities for parallel processing in sync operations\n");
        report.push_str(
            "4. **Caching**: Add digest caching for frequently accessed journal states\n\n",
        );

        report
    }
}

// =============================================================================
// Realistic Performance Test Effects
// =============================================================================

#[derive(Debug, Clone)]
pub struct RealisticTestEffects {
    journals: Arc<Mutex<HashMap<DeviceId, Journal>>>,
    network_messages: Arc<Mutex<Vec<(DeviceId, Vec<u8>)>>>,
    tracker: Arc<PerformanceTracker>,
    current_time: Arc<Mutex<u64>>,
}

impl RealisticTestEffects {
    pub fn new(tracker: Arc<PerformanceTracker>) -> Self {
        Self {
            journals: Arc::new(Mutex::new(HashMap::new())),
            network_messages: Arc::new(Mutex::new(Vec::new())),
            tracker,
            current_time: Arc::new(Mutex::new(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
            )),
        }
    }

    pub fn with_journal(&self, device_id: DeviceId, journal: Journal) {
        // Estimate memory usage
        let memory_mb = estimate_journal_memory(&journal) / (1024 * 1024);
        self.tracker.record_memory_usage(memory_mb);
        self.journals.lock().unwrap().insert(device_id, journal);
    }
}

impl JournalEffects for RealisticTestEffects {
    async fn get_journal(&self) -> Result<Journal, AuraError> {
        let start = Instant::now();
        let device_id = DeviceId::new();
        let result = self
            .journals
            .lock()
            .unwrap()
            .get(&device_id)
            .cloned()
            .ok_or_else(|| AuraError::Storage("No journal found".to_string()));
        self.tracker.record_latency(start.elapsed());
        result
    }

    async fn update_journal(&self, journal: Journal) -> Result<(), AuraError> {
        let start = Instant::now();
        let device_id = DeviceId::new();
        let memory_mb = estimate_journal_memory(&journal) / (1024 * 1024);
        self.tracker.record_memory_usage(memory_mb);
        self.journals.lock().unwrap().insert(device_id, journal);
        self.tracker.record_latency(start.elapsed());
        Ok(())
    }
}

impl NetworkEffects for RealisticTestEffects {
    async fn send_message(&self, peer: DeviceId, data: Vec<u8>) -> Result<(), AuraError> {
        let start = Instant::now();

        // Simulate realistic network latency based on message size
        let latency = Duration::from_millis(10 + (data.len() / 1000) as u64);
        tokio::time::sleep(latency).await;

        self.network_messages.lock().unwrap().push((peer, data));
        self.tracker.record_latency(start.elapsed());
        Ok(())
    }

    async fn receive_message(&self, _timeout: Duration) -> Result<(DeviceId, Vec<u8>), AuraError> {
        let start = Instant::now();
        let mut messages = self.network_messages.lock().unwrap();
        let result = if !messages.is_empty() {
            Ok(messages.remove(0))
        } else {
            Err(AuraError::Network("No messages available".to_string()))
        };
        self.tracker.record_latency(start.elapsed());
        result
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

impl TimeEffects for RealisticTestEffects {
    async fn current_time(&self) -> u64 {
        *self.current_time.lock().unwrap()
    }

    async fn sleep(&self, duration: Duration) {
        tokio::time::sleep(duration).await;
    }
}

impl RandomEffects for RealisticTestEffects {
    async fn random_bytes(&self, length: usize) -> Vec<u8> {
        (0..length).map(|i| (i % 256) as u8).collect()
    }

    async fn random_u64(&self) -> u64 {
        42 // Deterministic for baselines
    }
}

// =============================================================================
// Utility Functions
// =============================================================================

fn estimate_journal_memory(journal: &Journal) -> usize {
    let mut size = std::mem::size_of::<Journal>();

    for (key, value) in &journal.facts {
        size += key.len();
        size += match value {
            FactValue::String(s) => s.len(),
            FactValue::Number(_) => 8,
            FactValue::Boolean(_) => 1,
        };
    }

    size += journal.caps.len() * 64; // Rough estimate
    size
}

fn create_benchmark_journal(op_count: usize) -> Journal {
    let mut journal = Journal::new();

    for i in 0..op_count {
        let key = format!("benchmark_operation_{}", i);
        let value = FactValue::String(format!("realistic_payload_data_{}_with_content", i));
        journal.facts.insert(key, value);
    }

    journal
}

// =============================================================================
// Baseline Performance Tests
// =============================================================================

fn establish_anti_entropy_baseline(c: &mut Criterion) {
    let tracker = Arc::new(PerformanceTracker::new());

    c.bench_function("anti_entropy_baseline", |b| {
        b.to_async(tokio::runtime::Runtime::new().unwrap())
            .iter(|| async {
                tracker.clear_samples();

                let effects = RealisticTestEffects::new(tracker.clone());
                let journal = create_benchmark_journal(500);
                let device_id = DeviceId::new();
                let peer_id = DeviceId::new();

                effects.with_journal(device_id, journal.clone());
                effects.with_journal(peer_id, journal);

                let protocol = AntiEntropyProtocol::new(AntiEntropyConfig::default());

                let start = Instant::now();
                let _result = protocol.execute(&effects, peer_id).await;
                tracker.record_latency(start.elapsed());

                black_box(())
            });
    });

    // Create and store baseline
    let baseline = tracker.create_baseline("AntiEntropy", "standard_500_ops");
    tracker
        .baselines
        .lock()
        .unwrap()
        .insert("anti_entropy_standard".to_string(), baseline);
}

fn establish_journal_sync_baseline(c: &mut Criterion) {
    let tracker = Arc::new(PerformanceTracker::new());

    c.bench_function("journal_sync_baseline", |b| {
        b.to_async(tokio::runtime::Runtime::new().unwrap())
            .iter(|| async {
                tracker.clear_samples();

                let effects = RealisticTestEffects::new(tracker.clone());
                let journal = create_benchmark_journal(300);
                let device_id = DeviceId::new();
                let peers = vec![DeviceId::new(), DeviceId::new(), DeviceId::new()];

                effects.with_journal(device_id, journal.clone());
                for peer in &peers {
                    effects.with_journal(*peer, journal.clone());
                }

                let protocol = JournalSyncProtocol::new(JournalSyncConfig::default());

                let start = Instant::now();
                let _result = protocol.sync_with_peers(&effects, peers).await;
                tracker.record_latency(start.elapsed());

                black_box(())
            });
    });

    let baseline = tracker.create_baseline("JournalSync", "multi_peer_300_ops");
    tracker
        .baselines
        .lock()
        .unwrap()
        .insert("journal_sync_standard".to_string(), baseline);
}

fn establish_snapshot_baseline(c: &mut Criterion) {
    let tracker = Arc::new(PerformanceTracker::new());

    c.bench_function("snapshot_protocol_baseline", |b| {
        b.to_async(tokio::runtime::Runtime::new().unwrap())
            .iter(|| async {
                tracker.clear_samples();

                let effects = RealisticTestEffects::new(tracker.clone());
                let journal = create_benchmark_journal(1000);
                let device_id = DeviceId::new();
                let participants = vec![
                    DeviceId::new(),
                    DeviceId::new(),
                    DeviceId::new(),
                    DeviceId::new(),
                ];

                effects.with_journal(device_id, journal.clone());
                for participant in &participants {
                    effects.with_journal(*participant, journal.clone());
                }

                let protocol = SnapshotProtocol::new(SnapshotConfig::default());

                let start = Instant::now();
                let _result = protocol.coordinate_snapshot(&effects, participants).await;
                tracker.record_latency(start.elapsed());

                black_box(())
            });
    });

    let baseline = tracker.create_baseline("Snapshot", "coordination_1000_ops");
    tracker
        .baselines
        .lock()
        .unwrap()
        .insert("snapshot_standard".to_string(), baseline);
}

fn generate_performance_report(_c: &mut Criterion) {
    // This function would typically load stored baselines and compare with current performance
    // For demo purposes, we'll create sample data

    let tracker = Arc::new(PerformanceTracker::new());

    let sample_baseline = PerformanceBaseline {
        protocol_name: "AntiEntropy".to_string(),
        test_scenario: "standard_500_ops".to_string(),
        operations_per_second: 120.5,
        average_latency_ms: 8.3,
        p95_latency_ms: 15.2,
        p99_latency_ms: 28.1,
        memory_usage_mb: 45.2,
        network_bandwidth_mbps: 2.1,
        success_rate: 99.8,
        timestamp: 1640995200, // Example timestamp
    };

    let sample_current = PerformanceBaseline {
        protocol_name: "AntiEntropy".to_string(),
        test_scenario: "standard_500_ops".to_string(),
        operations_per_second: 135.2,
        average_latency_ms: 7.4,
        p95_latency_ms: 13.8,
        p99_latency_ms: 25.9,
        memory_usage_mb: 43.1,
        network_bandwidth_mbps: 2.3,
        success_rate: 99.9,
        timestamp: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    };

    let comparison = tracker.compare_with_baseline(sample_current, sample_baseline);
    let report = tracker.export_report(&[comparison]);

    // Write report to file
    if let Err(e) = fs::write("target/performance_report.md", &report) {
        eprintln!("Failed to write performance report: {}", e);
    } else {
        println!("Performance report written to target/performance_report.md");
    }

    println!("\n{}", report);
}

// =============================================================================
// Criterion Configuration
// =============================================================================

criterion_group!(
    performance_report_benches,
    establish_anti_entropy_baseline,
    establish_journal_sync_baseline,
    establish_snapshot_baseline,
    generate_performance_report,
);

criterion_main!(performance_report_benches);
