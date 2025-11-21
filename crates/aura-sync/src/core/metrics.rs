//! Unified metrics framework for aura-sync protocols
//!
//! This module provides a centralized metrics collection system that consolidates
//! all performance, operational, and resource metrics scattered across the aura-sync
//! crate into a single, observability-focused framework.

use aura_core::DeviceId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Unified metrics collector following observability best practices
#[derive(Debug, Clone)]
pub struct MetricsCollector {
    registry: Arc<MetricsRegistry>,
}

/// Central metrics registry implementing OpenTelemetry conventions
#[derive(Debug)]
struct MetricsRegistry {
    /// Operational counters for sync activities
    operational: Arc<Mutex<OperationalMetrics>>,
    /// Performance measurements with timing data
    performance: Arc<Mutex<PerformanceMetrics>>,
    /// Resource utilization tracking
    resources: Arc<Mutex<ResourceMetrics>>,
    /// Error tracking with categorization
    errors: Arc<Mutex<ErrorMetrics>>,
    /// Active timing measurements
    active_timers: Arc<Mutex<HashMap<String, u64>>>,
}

/// Operational metrics following Prometheus naming conventions
#[derive(Debug, Default)]
pub struct OperationalMetrics {
    /// Total sync sessions initiated
    pub sync_sessions_total: AtomicU64,
    /// Total sync sessions completed successfully
    pub sync_sessions_completed_total: AtomicU64,
    /// Total sync sessions failed
    pub sync_sessions_failed_total: AtomicU64,
    /// Total operations transferred across all sessions
    pub sync_operations_transferred_total: AtomicU64,
    /// Total bytes transferred during sync operations
    pub sync_bytes_transferred_total: AtomicU64,
    /// Currently active sync sessions
    pub active_sync_sessions: AtomicI64,
    /// Currently connected peers
    pub connected_peers: AtomicI64,
    /// Current queue depth for pending operations
    pub queue_depth: AtomicI64,
    /// Rate limit violations encountered
    pub rate_limit_violations_total: AtomicU64,
}

/// Performance metrics with timing distributions
#[derive(Debug, Default)]
pub struct PerformanceMetrics {
    /// Sync session duration histogram buckets
    pub sync_duration_histogram: HistogramMetric,
    /// Network latency histogram buckets
    pub network_latency_histogram: HistogramMetric,
    /// Operation processing time histogram
    pub operation_processing_histogram: HistogramMetric,
    /// Current operations per second rate
    pub operations_per_second: AtomicI64,
    /// Current bytes per second rate
    pub bytes_per_second: AtomicI64,
    /// Average sync duration in milliseconds
    pub average_sync_duration_ms: AtomicU64,
    /// Compression ratios achieved
    pub compression_ratio_histogram: HistogramMetric,
}

/// Resource utilization metrics
#[derive(Debug, Default)]
pub struct ResourceMetrics {
    /// CPU usage percentage (0-100)
    pub cpu_usage_percent: AtomicI64,
    /// Memory usage in bytes
    pub memory_usage_bytes: AtomicU64,
    /// Network bandwidth usage in bytes per second
    pub network_bandwidth_bps: AtomicU64,
    /// Peer connection pool size
    pub peer_connection_pool_size: AtomicI64,
    /// Message queue size across all queues
    pub message_queue_size: AtomicI64,
    /// Active timers count
    pub active_timers_count: AtomicI64,
}

/// Error metrics with detailed categorization
#[derive(Debug, Default)]
pub struct ErrorMetrics {
    /// Network communication errors
    pub network_errors_total: AtomicU64,
    /// Protocol-level errors
    pub protocol_errors_total: AtomicU64,
    /// Operation timeout errors
    pub timeout_errors_total: AtomicU64,
    /// Data validation errors
    pub validation_errors_total: AtomicU64,
    /// Resource exhaustion errors
    pub resource_errors_total: AtomicU64,
    /// Authorization/capability errors
    pub authorization_errors_total: AtomicU64,
    /// Current error rate percentage (calculated)
    pub error_rate_percent: AtomicI64,
}

/// Histogram metric implementation for latency distributions
#[derive(Debug)]
pub struct HistogramMetric {
    /// Histogram buckets with upper bounds and counts
    buckets: Arc<Mutex<Vec<HistogramBucket>>>,
    /// Total sum of all observed values
    sum: AtomicU64,
    /// Total count of all observations
    count: AtomicU64,
}

impl Default for HistogramMetric {
    fn default() -> Self {
        Self {
            buckets: Arc::new(Mutex::new(Self::default_buckets())),
            sum: AtomicU64::new(0),
            count: AtomicU64::new(0),
        }
    }
}

impl HistogramMetric {
    /// Create default latency buckets (ms): 1, 5, 10, 25, 50, 100, 250, 500, 1000, 2500, 5000, +Inf
    fn default_buckets() -> Vec<HistogramBucket> {
        [
            1.0,
            5.0,
            10.0,
            25.0,
            50.0,
            100.0,
            250.0,
            500.0,
            1000.0,
            2500.0,
            5000.0,
            f64::INFINITY,
        ]
        .iter()
        .map(|&upper_bound| HistogramBucket {
            upper_bound,
            count: AtomicU64::new(0),
        })
        .collect()
    }

    /// Observe a value in the histogram
    pub fn observe(&self, value: f64) {
        // Update sum and count
        self.sum.fetch_add(value as u64, Ordering::Relaxed);
        self.count.fetch_add(1, Ordering::Relaxed);

        // Update appropriate bucket
        if let Ok(buckets) = self.buckets.lock() {
            for bucket in buckets.iter() {
                if value <= bucket.upper_bound {
                    bucket.count.fetch_add(1, Ordering::Relaxed);
                    break;
                }
            }
        }
    }

    /// Get current histogram statistics
    pub fn stats(&self) -> HistogramStats {
        HistogramStats {
            sum: self.sum.load(Ordering::Relaxed),
            count: self.count.load(Ordering::Relaxed),
            buckets: if let Ok(buckets) = self.buckets.lock() {
                buckets
                    .iter()
                    .map(|b| (b.upper_bound, b.count.load(Ordering::Relaxed)))
                    .collect()
            } else {
                vec![]
            },
        }
    }
}

/// Histogram bucket with atomic counter
#[derive(Debug)]
pub struct HistogramBucket {
    /// Upper bound of this bucket
    pub upper_bound: f64,
    /// Atomic count of observations in this bucket
    pub count: AtomicU64,
}

/// Histogram statistics snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistogramStats {
    /// Sum of all observed values
    pub sum: u64,
    /// Total number of observations
    pub count: u64,
    /// Histogram buckets as (upper_bound, count) pairs
    pub buckets: Vec<(f64, u64)>,
}

impl HistogramStats {
    /// Calculate average value
    pub fn average(&self) -> f64 {
        if self.count == 0 {
            0.0
        } else {
            self.sum as f64 / self.count as f64
        }
    }

    /// Estimate percentile value (approximate)
    pub fn percentile(&self, p: f64) -> f64 {
        if self.buckets.is_empty() || self.count == 0 {
            return 0.0;
        }

        let target_count = (self.count as f64 * p / 100.0) as u64;
        let mut cumulative = 0u64;

        for (upper_bound, count) in &self.buckets {
            cumulative += count;
            if cumulative >= target_count {
                return *upper_bound;
            }
        }

        self.buckets.last().map(|(bound, _)| *bound).unwrap_or(0.0)
    }
}

/// Error categories for consistent classification
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ErrorCategory {
    /// Network-related errors
    Network,
    /// Protocol-specific errors
    Protocol,
    /// Timeout errors
    Timeout,
    /// Validation errors
    Validation,
    /// Resource exhaustion errors
    Resource,
    /// Authorization errors
    Authorization,
}

/// Comprehensive metrics snapshot for export
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncMetricsSnapshot {
    /// Operational metrics snapshot
    pub operational: OperationalSnapshot,
    /// Performance metrics snapshot
    pub performance: PerformanceSnapshot,
    /// Resource usage snapshot
    pub resources: ResourceSnapshot,
    /// Error metrics snapshot
    pub errors: ErrorSnapshot,
    /// Snapshot timestamp (Unix seconds)
    pub timestamp: u64,
}

/// Operational metrics snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationalSnapshot {
    pub sync_sessions_total: u64,
    pub sync_sessions_completed_total: u64,
    pub sync_sessions_failed_total: u64,
    pub sync_operations_transferred_total: u64,
    pub sync_bytes_transferred_total: u64,
    pub active_sync_sessions: i64,
    pub connected_peers: i64,
    pub queue_depth: i64,
    pub rate_limit_violations_total: u64,
    pub success_rate_percent: f64,
}

/// Performance metrics snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceSnapshot {
    pub sync_duration_stats: HistogramStats,
    pub network_latency_stats: HistogramStats,
    pub operation_processing_stats: HistogramStats,
    pub operations_per_second: i64,
    pub bytes_per_second: i64,
    pub average_sync_duration_ms: u64,
    pub compression_ratio_stats: HistogramStats,
}

/// Resource metrics snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceSnapshot {
    pub cpu_usage_percent: i64,
    pub memory_usage_bytes: u64,
    pub network_bandwidth_bps: u64,
    pub peer_connection_pool_size: i64,
    pub message_queue_size: i64,
    pub active_timers_count: i64,
}

/// Error metrics snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorSnapshot {
    pub network_errors_total: u64,
    pub protocol_errors_total: u64,
    pub timeout_errors_total: u64,
    pub validation_errors_total: u64,
    pub resource_errors_total: u64,
    pub authorization_errors_total: u64,
    pub error_rate_percent: i64,
    pub total_errors: u64,
}

impl MetricsCollector {
    /// Create a new metrics collector
    pub fn new() -> Self {
        Self {
            registry: Arc::new(MetricsRegistry {
                operational: Arc::new(Mutex::new(OperationalMetrics::default())),
                performance: Arc::new(Mutex::new(PerformanceMetrics::default())),
                resources: Arc::new(Mutex::new(ResourceMetrics::default())),
                errors: Arc::new(Mutex::new(ErrorMetrics::default())),
                active_timers: Arc::new(Mutex::new(HashMap::new())),
            }),
        }
    }

    /// Record sync session start
    ///
    /// Note: Callers should obtain `now` as Unix timestamp via TimeEffects and pass it to this method
    pub fn record_sync_start(&self, session_id: &str, now: u64) {
        if let Ok(operational) = self.registry.operational.lock() {
            operational
                .sync_sessions_total
                .fetch_add(1, Ordering::Relaxed);
            operational
                .active_sync_sessions
                .fetch_add(1, Ordering::Relaxed);
        }

        // Start timing this session
        if let Ok(mut timers) = self.registry.active_timers.lock() {
            timers.insert(format!("sync_session_{}", session_id), now);
        }
    }

    /// Record sync session completion
    ///
    /// Note: Callers should obtain `now` as Unix timestamp via TimeEffects and pass it to this method
    pub fn record_sync_completion(
        &self,
        session_id: &str,
        ops_transferred: usize,
        bytes_transferred: usize,
        now: u64,
    ) {
        let duration = if let Ok(mut timers) = self.registry.active_timers.lock() {
            timers
                .remove(&format!("sync_session_{}", session_id))
                .map(|start| {
                    let elapsed_secs = now.saturating_sub(start);
                    Duration::from_secs(elapsed_secs)
                })
                .unwrap_or(Duration::ZERO)
        } else {
            Duration::ZERO
        };

        if let Ok(operational) = self.registry.operational.lock() {
            operational
                .sync_sessions_completed_total
                .fetch_add(1, Ordering::Relaxed);
            operational
                .active_sync_sessions
                .fetch_sub(1, Ordering::Relaxed);
            operational
                .sync_operations_transferred_total
                .fetch_add(ops_transferred as u64, Ordering::Relaxed);
            operational
                .sync_bytes_transferred_total
                .fetch_add(bytes_transferred as u64, Ordering::Relaxed);
        }

        if let Ok(performance) = self.registry.performance.lock() {
            performance
                .sync_duration_histogram
                .observe(duration.as_millis() as f64);

            // Update average duration (simple moving average)
            let current_avg = performance.average_sync_duration_ms.load(Ordering::Relaxed);
            let new_avg = if current_avg == 0 {
                duration.as_millis() as u64
            } else {
                (current_avg * 9 + duration.as_millis() as u64) / 10 // Simple moving average
            };
            performance
                .average_sync_duration_ms
                .store(new_avg, Ordering::Relaxed);
        }
    }

    /// Record sync session failure
    pub fn record_sync_failure(&self, session_id: &str, category: ErrorCategory, details: &str) {
        // Remove timer and update counters
        if let Ok(mut timers) = self.registry.active_timers.lock() {
            timers.remove(&format!("sync_session_{}", session_id));
        }

        if let Ok(operational) = self.registry.operational.lock() {
            operational
                .sync_sessions_failed_total
                .fetch_add(1, Ordering::Relaxed);
            operational
                .active_sync_sessions
                .fetch_sub(1, Ordering::Relaxed);
        }

        self.record_error(category, details);
    }

    /// Record an error by category
    pub fn record_error(&self, category: ErrorCategory, _details: &str) {
        if let Ok(errors) = self.registry.errors.lock() {
            match category {
                ErrorCategory::Network => {
                    errors.network_errors_total.fetch_add(1, Ordering::Relaxed);
                }
                ErrorCategory::Protocol => {
                    errors.protocol_errors_total.fetch_add(1, Ordering::Relaxed);
                }
                ErrorCategory::Timeout => {
                    errors.timeout_errors_total.fetch_add(1, Ordering::Relaxed);
                }
                ErrorCategory::Validation => {
                    errors
                        .validation_errors_total
                        .fetch_add(1, Ordering::Relaxed);
                }
                ErrorCategory::Resource => {
                    errors.resource_errors_total.fetch_add(1, Ordering::Relaxed);
                }
                ErrorCategory::Authorization => {
                    errors
                        .authorization_errors_total
                        .fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    }

    /// Record network latency measurement
    pub fn record_network_latency(&self, peer: DeviceId, latency: Duration) {
        if let Ok(performance) = self.registry.performance.lock() {
            performance
                .network_latency_histogram
                .observe(latency.as_millis() as f64);
        }
    }

    /// Record operation processing time
    pub fn record_operation_processing_time(&self, operation: &str, duration: Duration) {
        if let Ok(performance) = self.registry.performance.lock() {
            performance
                .operation_processing_histogram
                .observe(duration.as_micros() as f64);
        }
    }

    /// Record compression ratio achieved
    pub fn record_compression_ratio(&self, ratio: f32) {
        if let Ok(performance) = self.registry.performance.lock() {
            performance
                .compression_ratio_histogram
                .observe(ratio as f64);
        }
    }

    /// Update resource usage metrics
    pub fn update_resource_usage(&self, cpu_percent: u32, memory_bytes: u64, network_bps: u64) {
        if let Ok(resources) = self.registry.resources.lock() {
            resources
                .cpu_usage_percent
                .store(cpu_percent as i64, Ordering::Relaxed);
            resources
                .memory_usage_bytes
                .store(memory_bytes, Ordering::Relaxed);
            resources
                .network_bandwidth_bps
                .store(network_bps, Ordering::Relaxed);
        }
    }

    /// Update peer connection count
    pub fn update_peer_count(&self, count: usize) {
        if let Ok(operational) = self.registry.operational.lock() {
            operational
                .connected_peers
                .store(count as i64, Ordering::Relaxed);
        }
    }

    /// Update queue depth
    pub fn update_queue_depth(&self, depth: usize) {
        if let Ok(operational) = self.registry.operational.lock() {
            operational
                .queue_depth
                .store(depth as i64, Ordering::Relaxed);
        }
    }

    /// Record rate limit violation
    pub fn record_rate_limit_violation(&self, peer: DeviceId) {
        if let Ok(operational) = self.registry.operational.lock() {
            operational
                .rate_limit_violations_total
                .fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Export comprehensive metrics snapshot
    pub fn export_snapshot(&self) -> SyncMetricsSnapshot {
        let operational_snapshot = if let Ok(operational) = self.registry.operational.lock() {
            let total_sessions = operational.sync_sessions_total.load(Ordering::Relaxed);
            let completed_sessions = operational
                .sync_sessions_completed_total
                .load(Ordering::Relaxed);
            let success_rate = if total_sessions > 0 {
                (completed_sessions as f64 / total_sessions as f64) * 100.0
            } else {
                100.0
            };

            OperationalSnapshot {
                sync_sessions_total: total_sessions,
                sync_sessions_completed_total: completed_sessions,
                sync_sessions_failed_total: operational
                    .sync_sessions_failed_total
                    .load(Ordering::Relaxed),
                sync_operations_transferred_total: operational
                    .sync_operations_transferred_total
                    .load(Ordering::Relaxed),
                sync_bytes_transferred_total: operational
                    .sync_bytes_transferred_total
                    .load(Ordering::Relaxed),
                active_sync_sessions: operational.active_sync_sessions.load(Ordering::Relaxed),
                connected_peers: operational.connected_peers.load(Ordering::Relaxed),
                queue_depth: operational.queue_depth.load(Ordering::Relaxed),
                rate_limit_violations_total: operational
                    .rate_limit_violations_total
                    .load(Ordering::Relaxed),
                success_rate_percent: success_rate,
            }
        } else {
            OperationalSnapshot {
                sync_sessions_total: 0,
                sync_sessions_completed_total: 0,
                sync_sessions_failed_total: 0,
                sync_operations_transferred_total: 0,
                sync_bytes_transferred_total: 0,
                active_sync_sessions: 0,
                connected_peers: 0,
                queue_depth: 0,
                rate_limit_violations_total: 0,
                success_rate_percent: 100.0,
            }
        };

        let performance_snapshot = if let Ok(performance) = self.registry.performance.lock() {
            PerformanceSnapshot {
                sync_duration_stats: performance.sync_duration_histogram.stats(),
                network_latency_stats: performance.network_latency_histogram.stats(),
                operation_processing_stats: performance.operation_processing_histogram.stats(),
                operations_per_second: performance.operations_per_second.load(Ordering::Relaxed),
                bytes_per_second: performance.bytes_per_second.load(Ordering::Relaxed),
                average_sync_duration_ms: performance
                    .average_sync_duration_ms
                    .load(Ordering::Relaxed),
                compression_ratio_stats: performance.compression_ratio_histogram.stats(),
            }
        } else {
            PerformanceSnapshot {
                sync_duration_stats: HistogramStats {
                    sum: 0,
                    count: 0,
                    buckets: vec![],
                },
                network_latency_stats: HistogramStats {
                    sum: 0,
                    count: 0,
                    buckets: vec![],
                },
                operation_processing_stats: HistogramStats {
                    sum: 0,
                    count: 0,
                    buckets: vec![],
                },
                operations_per_second: 0,
                bytes_per_second: 0,
                average_sync_duration_ms: 0,
                compression_ratio_stats: HistogramStats {
                    sum: 0,
                    count: 0,
                    buckets: vec![],
                },
            }
        };

        let resources_snapshot = if let Ok(resources) = self.registry.resources.lock() {
            ResourceSnapshot {
                cpu_usage_percent: resources.cpu_usage_percent.load(Ordering::Relaxed),
                memory_usage_bytes: resources.memory_usage_bytes.load(Ordering::Relaxed),
                network_bandwidth_bps: resources.network_bandwidth_bps.load(Ordering::Relaxed),
                peer_connection_pool_size: resources
                    .peer_connection_pool_size
                    .load(Ordering::Relaxed),
                message_queue_size: resources.message_queue_size.load(Ordering::Relaxed),
                active_timers_count: resources.active_timers_count.load(Ordering::Relaxed),
            }
        } else {
            ResourceSnapshot {
                cpu_usage_percent: 0,
                memory_usage_bytes: 0,
                network_bandwidth_bps: 0,
                peer_connection_pool_size: 0,
                message_queue_size: 0,
                active_timers_count: 0,
            }
        };

        let errors_snapshot = if let Ok(errors) = self.registry.errors.lock() {
            let total_errors = errors.network_errors_total.load(Ordering::Relaxed)
                + errors.protocol_errors_total.load(Ordering::Relaxed)
                + errors.timeout_errors_total.load(Ordering::Relaxed)
                + errors.validation_errors_total.load(Ordering::Relaxed)
                + errors.resource_errors_total.load(Ordering::Relaxed)
                + errors.authorization_errors_total.load(Ordering::Relaxed);

            ErrorSnapshot {
                network_errors_total: errors.network_errors_total.load(Ordering::Relaxed),
                protocol_errors_total: errors.protocol_errors_total.load(Ordering::Relaxed),
                timeout_errors_total: errors.timeout_errors_total.load(Ordering::Relaxed),
                validation_errors_total: errors.validation_errors_total.load(Ordering::Relaxed),
                resource_errors_total: errors.resource_errors_total.load(Ordering::Relaxed),
                authorization_errors_total: errors
                    .authorization_errors_total
                    .load(Ordering::Relaxed),
                error_rate_percent: errors.error_rate_percent.load(Ordering::Relaxed),
                total_errors,
            }
        } else {
            ErrorSnapshot {
                network_errors_total: 0,
                protocol_errors_total: 0,
                timeout_errors_total: 0,
                validation_errors_total: 0,
                resource_errors_total: 0,
                authorization_errors_total: 0,
                error_rate_percent: 0,
                total_errors: 0,
            }
        };

        SyncMetricsSnapshot {
            operational: operational_snapshot,
            performance: performance_snapshot,
            resources: resources_snapshot,
            errors: errors_snapshot,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        }
    }

    /// Export metrics in Prometheus format
    pub fn export_prometheus(&self) -> String {
        let snapshot = self.export_snapshot();

        format!(
            "# HELP aura_sync_sessions_total Total number of sync sessions initiated\n\
            # TYPE aura_sync_sessions_total counter\n\
            aura_sync_sessions_total {}\n\
            # HELP aura_sync_sessions_completed_total Total number of sync sessions completed successfully\n\
            # TYPE aura_sync_sessions_completed_total counter\n\
            aura_sync_sessions_completed_total {}\n\
            # HELP aura_sync_active_sessions Current number of active sync sessions\n\
            # TYPE aura_sync_active_sessions gauge\n\
            aura_sync_active_sessions {}\n\
            # HELP aura_sync_operations_transferred_total Total number of operations transferred\n\
            # TYPE aura_sync_operations_transferred_total counter\n\
            aura_sync_operations_transferred_total {}\n\
            # HELP aura_sync_bytes_transferred_total Total bytes transferred during sync\n\
            # TYPE aura_sync_bytes_transferred_total counter\n\
            aura_sync_bytes_transferred_total {}\n\
            # HELP aura_sync_success_rate_percent Success rate percentage\n\
            # TYPE aura_sync_success_rate_percent gauge\n\
            aura_sync_success_rate_percent {}\n\
            # HELP aura_sync_average_duration_ms Average sync duration in milliseconds\n\
            # TYPE aura_sync_average_duration_ms gauge\n\
            aura_sync_average_duration_ms {}\n\
            # HELP aura_sync_errors_total Total number of errors by category\n\
            # TYPE aura_sync_errors_total counter\n\
            aura_sync_errors_total{{category=\"network\"}} {}\n\
            aura_sync_errors_total{{category=\"protocol\"}} {}\n\
            aura_sync_errors_total{{category=\"timeout\"}} {}\n\
            aura_sync_errors_total{{category=\"validation\"}} {}\n",
            snapshot.operational.sync_sessions_total,
            snapshot.operational.sync_sessions_completed_total,
            snapshot.operational.active_sync_sessions,
            snapshot.operational.sync_operations_transferred_total,
            snapshot.operational.sync_bytes_transferred_total,
            snapshot.operational.success_rate_percent,
            snapshot.performance.average_sync_duration_ms,
            snapshot.errors.network_errors_total,
            snapshot.errors.protocol_errors_total,
            snapshot.errors.timeout_errors_total,
            snapshot.errors.validation_errors_total
        )
    }
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

/// Type alias for the main metrics interface used across aura-sync
pub type SyncMetrics = MetricsCollector;

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn test_metrics_collector_creation() {
        let collector = MetricsCollector::new();
        let snapshot = collector.export_snapshot();

        assert_eq!(snapshot.operational.sync_sessions_total, 0);
        assert_eq!(snapshot.operational.connected_peers, 0);
        assert_eq!(snapshot.errors.total_errors, 0);
    }

    #[test]
    fn test_sync_session_lifecycle() {
        let collector = MetricsCollector::new();
        let now = 1000000u64; // Test timestamp

        collector.record_sync_start("test_session_1", now);
        let snapshot1 = collector.export_snapshot();
        assert_eq!(snapshot1.operational.sync_sessions_total, 1);
        assert_eq!(snapshot1.operational.active_sync_sessions, 1);

        collector.record_sync_completion("test_session_1", 50, 1024, now + 100);
        let snapshot2 = collector.export_snapshot();
        assert_eq!(snapshot2.operational.sync_sessions_completed_total, 1);
        assert_eq!(snapshot2.operational.active_sync_sessions, 0);
        assert_eq!(snapshot2.operational.sync_operations_transferred_total, 50);
        assert_eq!(snapshot2.operational.sync_bytes_transferred_total, 1024);
    }

    #[test]
    fn test_error_recording() {
        let collector = MetricsCollector::new();

        collector.record_error(ErrorCategory::Network, "Connection failed");
        collector.record_error(ErrorCategory::Protocol, "Invalid message");
        collector.record_error(ErrorCategory::Timeout, "Operation timed out");

        let snapshot = collector.export_snapshot();
        assert_eq!(snapshot.errors.network_errors_total, 1);
        assert_eq!(snapshot.errors.protocol_errors_total, 1);
        assert_eq!(snapshot.errors.timeout_errors_total, 1);
        assert_eq!(snapshot.errors.total_errors, 3);
    }

    #[test]
    fn test_histogram_functionality() {
        let histogram = HistogramMetric::default();

        histogram.observe(50.0);
        histogram.observe(150.0);
        histogram.observe(1500.0);

        let stats = histogram.stats();
        assert_eq!(stats.count, 3);
        assert_eq!(stats.sum, 1700);
        assert_eq!(stats.average(), 1700.0 / 3.0);
    }

    #[test]
    fn test_prometheus_export() {
        let collector = MetricsCollector::new();
        let now = 1000000u64;
        collector.record_sync_start("test", now);
        collector.record_sync_completion("test", 10, 100, now + 50);

        let prometheus_output = collector.export_prometheus();
        assert!(prometheus_output.contains("aura_sync_sessions_total 1"));
        assert!(prometheus_output.contains("aura_sync_sessions_completed_total 1"));
        assert!(prometheus_output.contains("aura_sync_operations_transferred_total 10"));
    }

    #[test]
    fn test_concurrent_access() {
        let collector = Arc::new(MetricsCollector::new());
        let mut handles = vec![];

        for i in 0..10 {
            let collector_clone = collector.clone();
            let handle = thread::spawn(move || {
                let now = 1000000u64 + i as u64;
                collector_clone.record_sync_start(&format!("session_{}", i), now);
                collector_clone.record_sync_completion(&format!("session_{}", i), i, i * 100, now + 50);
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.join().unwrap();
        }

        let snapshot = collector.export_snapshot();
        assert_eq!(snapshot.operational.sync_sessions_total, 10);
        assert_eq!(snapshot.operational.sync_sessions_completed_total, 10);
        assert_eq!(snapshot.operational.active_sync_sessions, 0);
    }
}
