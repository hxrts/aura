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
use std::time::Duration;

/// Unified metrics collector following observability best practices
#[derive(Debug, Clone)]
pub struct MetricsCollector {
    registry: Arc<MetricsRegistry>,
}

/// Central metrics registry implementing OpenTelemetry conventions
#[derive(Debug)]
struct MetricsRegistry {
    /// Operational counters for sync activities
    operational: Mutex<OperationalMetrics>,
    /// Performance measurements with timing data
    performance: Mutex<PerformanceMetrics>,
    /// Resource utilization tracking
    resources: Mutex<ResourceMetrics>,
    /// Error tracking with categorization
    errors: Mutex<ErrorMetrics>,
    /// Active timing measurements
    active_timers: Mutex<HashMap<String, u64>>,
    /// Last successful sync timestamp (ms since Unix epoch)
    last_sync_timestamp_ms: AtomicU64,
    /// Last operation timestamp (ms since Unix epoch)
    last_operation_timestamp_ms: AtomicU64,
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
    buckets: Mutex<Vec<HistogramBucket>>,
    /// Total sum of all observed values
    sum: AtomicU64,
    /// Total count of all observations
    count: AtomicU64,
}

impl Default for HistogramMetric {
    fn default() -> Self {
        Self {
            buckets: Mutex::new(Self::default_buckets()),
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
    /// Total number of sync sessions initiated
    pub sync_sessions_total: u64,
    /// Number of successfully completed sync sessions
    pub sync_sessions_completed_total: u64,
    /// Number of failed sync sessions
    pub sync_sessions_failed_total: u64,
    /// Total operations transferred across all sessions
    pub sync_operations_transferred_total: u64,
    /// Total bytes transferred across all sync sessions
    pub sync_bytes_transferred_total: u64,
    /// Currently active sync sessions
    pub active_sync_sessions: i64,
    /// Currently connected peer count
    pub connected_peers: i64,
    /// Current queue depth (pending operations)
    pub queue_depth: i64,
    /// Total number of rate limit violations
    pub rate_limit_violations_total: u64,
    /// Overall success rate percentage
    pub success_rate_percent: f64,
}

impl Default for OperationalSnapshot {
    fn default() -> Self {
        Self {
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
    }
}

/// Performance metrics snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceSnapshot {
    /// Histogram of sync duration statistics
    pub sync_duration_stats: HistogramStats,
    /// Histogram of network latency statistics
    pub network_latency_stats: HistogramStats,
    /// Histogram of operation processing time statistics
    pub operation_processing_stats: HistogramStats,
    /// Operations processed per second
    pub operations_per_second: i64,
    /// Bytes transferred per second
    pub bytes_per_second: i64,
    /// Average sync duration in milliseconds
    pub average_sync_duration_ms: u64,
    /// Histogram of compression ratio statistics
    pub compression_ratio_stats: HistogramStats,
}

impl Default for PerformanceSnapshot {
    fn default() -> Self {
        Self {
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
    }
}

/// Resource metrics snapshot
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ResourceSnapshot {
    /// CPU usage percentage
    pub cpu_usage_percent: i64,
    /// Memory usage in bytes
    pub memory_usage_bytes: u64,
    /// Network bandwidth in bits per second
    pub network_bandwidth_bps: u64,
    /// Peer connection pool size
    pub peer_connection_pool_size: i64,
    /// Message queue size
    pub message_queue_size: i64,
    /// Number of active timers
    pub active_timers_count: i64,
}

/// Error metrics snapshot
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ErrorSnapshot {
    /// Total network errors encountered
    pub network_errors_total: u64,
    /// Total protocol-level errors
    pub protocol_errors_total: u64,
    /// Total timeout errors
    pub timeout_errors_total: u64,
    /// Total validation errors
    pub validation_errors_total: u64,
    /// Total resource availability errors
    pub resource_errors_total: u64,
    /// Total authorization errors
    pub authorization_errors_total: u64,
    /// Error rate as a percentage
    pub error_rate_percent: i64,
    /// Total errors across all categories
    pub total_errors: u64,
}

fn sync_session_timer_key(session_id: &str) -> String {
    format!("sync_session_{session_id}")
}

fn with_locked<T, R>(mutex: &Mutex<T>, f: impl FnOnce(&mut T) -> R) -> Option<R> {
    let mut guard = mutex.lock().ok()?;
    Some(f(&mut guard))
}

fn load_u64(counter: &AtomicU64) -> u64 {
    counter.load(Ordering::Relaxed)
}

fn load_i64(counter: &AtomicI64) -> i64 {
    counter.load(Ordering::Relaxed)
}

impl MetricsRegistry {
    fn new() -> Self {
        Self {
            operational: Mutex::new(OperationalMetrics::default()),
            performance: Mutex::new(PerformanceMetrics::default()),
            resources: Mutex::new(ResourceMetrics::default()),
            errors: Mutex::new(ErrorMetrics::default()),
            active_timers: Mutex::new(HashMap::new()),
            last_sync_timestamp_ms: AtomicU64::new(0),
            last_operation_timestamp_ms: AtomicU64::new(0),
        }
    }

    fn operational_snapshot(&self) -> OperationalSnapshot {
        with_locked(&self.operational, |operational| {
            let total_sessions = load_u64(&operational.sync_sessions_total);
            let completed_sessions = load_u64(&operational.sync_sessions_completed_total);
            let success_rate_percent = if total_sessions > 0 {
                (completed_sessions as f64 / total_sessions as f64) * 100.0
            } else {
                100.0
            };

            OperationalSnapshot {
                sync_sessions_total: total_sessions,
                sync_sessions_completed_total: completed_sessions,
                sync_sessions_failed_total: load_u64(&operational.sync_sessions_failed_total),
                sync_operations_transferred_total: load_u64(
                    &operational.sync_operations_transferred_total,
                ),
                sync_bytes_transferred_total: load_u64(&operational.sync_bytes_transferred_total),
                active_sync_sessions: load_i64(&operational.active_sync_sessions),
                connected_peers: load_i64(&operational.connected_peers),
                queue_depth: load_i64(&operational.queue_depth),
                rate_limit_violations_total: load_u64(&operational.rate_limit_violations_total),
                success_rate_percent,
            }
        })
        .unwrap_or_default()
    }

    fn performance_snapshot(&self) -> PerformanceSnapshot {
        with_locked(&self.performance, |performance| PerformanceSnapshot {
            sync_duration_stats: performance.sync_duration_histogram.stats(),
            network_latency_stats: performance.network_latency_histogram.stats(),
            operation_processing_stats: performance.operation_processing_histogram.stats(),
            operations_per_second: load_i64(&performance.operations_per_second),
            bytes_per_second: load_i64(&performance.bytes_per_second),
            average_sync_duration_ms: load_u64(&performance.average_sync_duration_ms),
            compression_ratio_stats: performance.compression_ratio_histogram.stats(),
        })
        .unwrap_or_default()
    }

    fn resource_snapshot(&self) -> ResourceSnapshot {
        with_locked(&self.resources, |resources| ResourceSnapshot {
            cpu_usage_percent: load_i64(&resources.cpu_usage_percent),
            memory_usage_bytes: load_u64(&resources.memory_usage_bytes),
            network_bandwidth_bps: load_u64(&resources.network_bandwidth_bps),
            peer_connection_pool_size: load_i64(&resources.peer_connection_pool_size),
            message_queue_size: load_i64(&resources.message_queue_size),
            active_timers_count: load_i64(&resources.active_timers_count),
        })
        .unwrap_or_default()
    }

    fn error_snapshot(&self) -> ErrorSnapshot {
        with_locked(&self.errors, |errors| {
            let network_errors_total = load_u64(&errors.network_errors_total);
            let protocol_errors_total = load_u64(&errors.protocol_errors_total);
            let timeout_errors_total = load_u64(&errors.timeout_errors_total);
            let validation_errors_total = load_u64(&errors.validation_errors_total);
            let resource_errors_total = load_u64(&errors.resource_errors_total);
            let authorization_errors_total = load_u64(&errors.authorization_errors_total);

            ErrorSnapshot {
                network_errors_total,
                protocol_errors_total,
                timeout_errors_total,
                validation_errors_total,
                resource_errors_total,
                authorization_errors_total,
                error_rate_percent: load_i64(&errors.error_rate_percent),
                total_errors: network_errors_total
                    + protocol_errors_total
                    + timeout_errors_total
                    + validation_errors_total
                    + resource_errors_total
                    + authorization_errors_total,
            }
        })
        .unwrap_or_default()
    }
}

impl MetricsCollector {
    /// Create a new metrics collector
    pub fn new() -> Self {
        Self {
            registry: Arc::new(MetricsRegistry::new()),
        }
    }

    /// Record sync session start
    ///
    /// Note: Callers should obtain `now` as Unix timestamp via their time provider and pass it to this method
    pub fn record_sync_start(&self, session_id: &str, now: u64) {
        with_locked(&self.registry.operational, |operational| {
            operational
                .sync_sessions_total
                .fetch_add(1, Ordering::Relaxed);
            operational
                .active_sync_sessions
                .fetch_add(1, Ordering::Relaxed);
        });

        // Start timing this session
        with_locked(&self.registry.active_timers, |timers| {
            timers.insert(sync_session_timer_key(session_id), now);
        });
    }

    /// Record sync session completion
    ///
    /// Note: Callers should obtain `now` as Unix timestamp via their time provider and pass it to this method
    pub fn record_sync_completion(
        &self,
        session_id: &str,
        ops_transferred: u64,
        bytes_transferred: u64,
        now: u64,
    ) {
        let duration = with_locked(&self.registry.active_timers, |timers| {
            timers
                .remove(&sync_session_timer_key(session_id))
                .map(|start| Duration::from_millis(now.saturating_sub(start)))
                .unwrap_or(Duration::ZERO)
        })
        .unwrap_or(Duration::ZERO);

        with_locked(&self.registry.operational, |operational| {
            operational
                .sync_sessions_completed_total
                .fetch_add(1, Ordering::Relaxed);
            operational
                .active_sync_sessions
                .fetch_sub(1, Ordering::Relaxed);
            operational
                .sync_operations_transferred_total
                .fetch_add(ops_transferred, Ordering::Relaxed);
            operational
                .sync_bytes_transferred_total
                .fetch_add(bytes_transferred, Ordering::Relaxed);
        });

        with_locked(&self.registry.performance, |performance| {
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
        });
    }

    /// Record sync session failure
    pub fn record_sync_failure(&self, session_id: &str, category: ErrorCategory, details: &str) {
        // Remove timer and update counters
        with_locked(&self.registry.active_timers, |timers| {
            timers.remove(&sync_session_timer_key(session_id));
        });

        with_locked(&self.registry.operational, |operational| {
            operational
                .sync_sessions_failed_total
                .fetch_add(1, Ordering::Relaxed);
            operational
                .active_sync_sessions
                .fetch_sub(1, Ordering::Relaxed);
        });

        self.record_error(category, details);
    }

    /// Record an error by category
    pub fn record_error(&self, category: ErrorCategory, _details: &str) {
        with_locked(&self.registry.errors, |errors| match category {
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
        });
    }

    /// Record network latency measurement
    pub fn record_network_latency(&self, _peer: DeviceId, latency: Duration) {
        with_locked(&self.registry.performance, |performance| {
            performance
                .network_latency_histogram
                .observe(latency.as_millis() as f64);
        });
    }

    /// Record operation processing time
    pub fn record_operation_processing_time(&self, _operation: &str, duration: Duration) {
        with_locked(&self.registry.performance, |performance| {
            performance
                .operation_processing_histogram
                .observe(duration.as_micros() as f64);
        });
    }

    /// Record compression ratio achieved
    pub fn record_compression_ratio(&self, ratio: f32) {
        with_locked(&self.registry.performance, |performance| {
            performance
                .compression_ratio_histogram
                .observe(ratio as f64);
        });
    }

    /// Update resource usage metrics
    pub fn update_resource_usage(&self, cpu_percent: u32, memory_bytes: u64, network_bps: u64) {
        with_locked(&self.registry.resources, |resources| {
            resources
                .cpu_usage_percent
                .store(cpu_percent as i64, Ordering::Relaxed);
            resources
                .memory_usage_bytes
                .store(memory_bytes, Ordering::Relaxed);
            resources
                .network_bandwidth_bps
                .store(network_bps, Ordering::Relaxed);
        });
    }

    /// Update peer connection count
    pub fn update_peer_count(&self, count: u64) {
        with_locked(&self.registry.operational, |operational| {
            operational
                .connected_peers
                .store(count as i64, Ordering::Relaxed);
        });
    }

    /// Update queue depth
    pub fn update_queue_depth(&self, depth: u64) {
        with_locked(&self.registry.operational, |operational| {
            operational
                .queue_depth
                .store(depth as i64, Ordering::Relaxed);
        });
    }

    /// Record rate limit violation
    pub fn record_rate_limit_violation(&self, _peer: DeviceId) {
        with_locked(&self.registry.operational, |operational| {
            operational
                .rate_limit_violations_total
                .fetch_add(1, Ordering::Relaxed);
        });
    }

    /// Increment sync attempts for a peer
    pub fn increment_sync_attempts(&self, _peer: DeviceId) {
        with_locked(&self.registry.operational, |operational| {
            operational
                .sync_sessions_total
                .fetch_add(1, Ordering::Relaxed);
        });
    }

    /// Increment sync successes for a peer
    pub fn increment_sync_successes(&self, _peer: DeviceId) {
        with_locked(&self.registry.operational, |operational| {
            operational
                .sync_sessions_completed_total
                .fetch_add(1, Ordering::Relaxed);
        });
    }

    /// Add synced operations count for a peer
    pub fn add_synced_operations(&self, _peer: DeviceId, ops_count: u64) {
        with_locked(&self.registry.operational, |operational| {
            operational
                .sync_operations_transferred_total
                .fetch_add(ops_count, Ordering::Relaxed);
        });
    }

    /// Update last sync timestamp for a peer (callers provide wall-clock ms)
    pub fn update_last_sync(&self, _peer: DeviceId, timestamp_ms: u64) {
        self.registry
            .last_sync_timestamp_ms
            .store(timestamp_ms, Ordering::Relaxed);
    }

    /// Increment auto sync rounds counter
    pub fn increment_auto_sync_rounds(&self) {
        with_locked(&self.registry.operational, |operational| {
            operational
                .sync_sessions_total
                .fetch_add(1, Ordering::Relaxed);
        });
    }

    /// Add auto sync results
    pub fn add_auto_sync_results(&self, results: &[(DeviceId, u64)]) {
        with_locked(&self.registry.operational, |operational| {
            let mut total_ops = 0u64;
            for &(_, ops) in results {
                total_ops += ops;
            }
            operational
                .sync_operations_transferred_total
                .fetch_add(total_ops, Ordering::Relaxed);
        });
    }

    /// Update auto sync timing
    pub fn update_auto_sync_timing(&self, duration: Duration) {
        with_locked(&self.registry.performance, |performance| {
            performance
                .sync_duration_histogram
                .observe(duration.as_millis() as f64);
        });
    }

    /// Get last sync timestamp in milliseconds since Unix epoch
    pub fn get_last_sync_timestamp(&self) -> Option<u64> {
        let ts = self.registry.last_sync_timestamp_ms.load(Ordering::Relaxed);
        (ts > 0).then_some(ts)
    }

    /// Get total number of requests processed
    pub fn get_total_requests_processed(&self) -> u64 {
        with_locked(&self.registry.operational, |operational| {
            operational
                .sync_sessions_completed_total
                .load(Ordering::Relaxed)
        })
        .unwrap_or(0)
    }

    /// Get total number of errors encountered
    pub fn get_total_errors_encountered(&self) -> u64 {
        with_locked(&self.registry.errors, |errors| {
            errors.network_errors_total.load(Ordering::Relaxed)
                + errors.protocol_errors_total.load(Ordering::Relaxed)
                + errors.timeout_errors_total.load(Ordering::Relaxed)
                + errors.validation_errors_total.load(Ordering::Relaxed)
                + errors.resource_errors_total.load(Ordering::Relaxed)
                + errors.authorization_errors_total.load(Ordering::Relaxed)
        })
        .unwrap_or(0)
    }

    /// Get average sync latency in milliseconds
    pub fn get_average_sync_latency_ms(&self) -> f64 {
        with_locked(&self.registry.performance, |performance| {
            // Simple average calculation - in a real implementation this would use proper statistics
            performance.network_latency_histogram.stats().average()
        })
        .unwrap_or(0.0)
    }

    /// Get last operation timestamp in milliseconds since Unix epoch
    pub fn get_last_operation_timestamp(&self) -> Option<u64> {
        let ts = self
            .registry
            .last_operation_timestamp_ms
            .load(Ordering::Relaxed);
        (ts > 0).then_some(ts)
    }

    /// Export comprehensive metrics snapshot
    pub fn export_snapshot(&self, timestamp_secs: u64) -> SyncMetricsSnapshot {
        SyncMetricsSnapshot {
            operational: self.registry.operational_snapshot(),
            performance: self.registry.performance_snapshot(),
            resources: self.registry.resource_snapshot(),
            errors: self.registry.error_snapshot(),
            timestamp: timestamp_secs,
        }
    }

    /// Export metrics in Prometheus format
    pub fn export_prometheus(&self) -> String {
        let snapshot = self.export_snapshot(0);

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
        let snapshot = collector.export_snapshot(0);

        assert_eq!(snapshot.operational.sync_sessions_total, 0);
        assert_eq!(snapshot.operational.connected_peers, 0);
        assert_eq!(snapshot.errors.total_errors, 0);
    }

    #[test]
    fn test_sync_session_lifecycle() {
        let collector = MetricsCollector::new();
        let now = 1000000u64; // Test timestamp

        collector.record_sync_start("test_session_1", now);
        let snapshot1 = collector.export_snapshot(now);
        assert_eq!(snapshot1.operational.sync_sessions_total, 1);
        assert_eq!(snapshot1.operational.active_sync_sessions, 1);

        collector.record_sync_completion("test_session_1", 50, 1024, now + 100);
        let snapshot2 = collector.export_snapshot(now + 100);
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

        let snapshot = collector.export_snapshot(0);
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
        let base_now = 1_000_000u64;
        thread::scope(|scope| {
            for i in 0..10 {
                let collector_clone = collector.clone();
                scope.spawn(move || {
                    let now = base_now + i as u64;
                    collector_clone.record_sync_start(&format!("session_{i}"), now);
                    collector_clone.record_sync_completion(
                        &format!("session_{i}"),
                        i as u64,
                        (i as u64) * 100,
                        now + 50,
                    );
                });
            }
        });

        let snapshot = collector.export_snapshot(base_now + 9 + 50);
        assert_eq!(snapshot.operational.sync_sessions_total, 10);
        assert_eq!(snapshot.operational.sync_sessions_completed_total, 10);
        assert_eq!(snapshot.operational.active_sync_sessions, 0);
    }
}
