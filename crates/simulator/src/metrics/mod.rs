//! Unified metrics framework for simulation components
//!
//! This module provides a centralized metrics collection system that eliminates
//! duplication across simulation components while providing rich performance
//! monitoring and analysis capabilities.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;

pub mod collector;
pub mod registry;
pub mod types;

pub use collector::{MetricsCollector, MetricsSnapshot};
pub use registry::{GlobalMetrics, MetricRegistry};
pub use types::{MetricCategory, MetricType, MetricValue, PerformanceCounter, TimeSeries};

/// Unified metrics collection interface
pub trait MetricsProvider {
    /// Record a counter metric
    fn counter(&self, name: &str, value: u64);

    /// Record a gauge metric
    fn gauge(&self, name: &str, value: f64);

    /// Record a histogram metric
    fn histogram(&self, name: &str, value: f64);

    /// Record a timer metric (duration in milliseconds)
    fn timer(&self, name: &str, duration_ms: u64);

    /// Record custom metric with metadata
    fn custom(&self, name: &str, value: MetricValue, metadata: HashMap<String, String>);
}

/// Timer guard that records duration when dropped
pub struct TimerGuard {
    metric_name: String,
    start_time: SystemTime,
    metrics: Arc<MetricsCollector>,
}

impl TimerGuard {
    pub fn new(name: String, metrics: Arc<MetricsCollector>) -> Self {
        Self {
            metric_name: name,
            start_time: SystemTime::now(),
            metrics,
        }
    }
}

impl Drop for TimerGuard {
    fn drop(&mut self) {
        if let Ok(duration) = self.start_time.elapsed() {
            self.metrics
                .timer(&self.metric_name, duration.as_millis() as u64);
        }
    }
}

/// Central metrics registry for simulation components
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationMetrics {
    /// Core simulation metrics
    pub simulation: SimulationCoreMetrics,
    /// Property monitoring metrics
    pub property_monitoring: PropertyMonitoringMetrics,
    /// Performance and resource metrics
    pub performance: PerformanceMetrics,
    /// Network simulation metrics
    pub network: NetworkMetrics,
    /// Protocol execution metrics
    pub protocol: ProtocolMetrics,
    /// Custom metrics
    pub custom: HashMap<String, MetricValue>,
}

/// Core simulation execution metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationCoreMetrics {
    /// Current simulation tick
    pub current_tick: u64,
    /// Current simulation time in milliseconds
    pub current_time: u64,
    /// Total simulation duration in milliseconds
    pub total_duration_ms: u64,
    /// Number of participants
    pub participant_count: usize,
    /// Events generated per tick (time series)
    pub events_per_tick: TimeSeries<u64>,
    /// Simulation state changes
    pub state_changes: u64,
    /// Checkpoint creation count
    pub checkpoints_created: u64,
    /// Time travel operations
    pub time_travel_operations: u64,
}

/// Property monitoring metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertyMonitoringMetrics {
    /// Total property evaluations
    pub total_evaluations: u64,
    /// Property evaluation time in milliseconds
    pub evaluation_time_ms: TimeSeries<u64>,
    /// Violations detected
    pub violations_detected: u64,
    /// Properties checked per evaluation
    pub properties_per_evaluation: TimeSeries<usize>,
    /// Evaluation success rate
    pub success_rate: f64,
    /// Confidence scores (time series)
    pub confidence_scores: TimeSeries<f64>,
    /// Memory usage for traces
    pub trace_memory_bytes: u64,
}

/// Performance and resource metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceMetrics {
    /// Memory usage in bytes
    pub memory_usage_bytes: u64,
    /// Peak memory usage in bytes
    pub peak_memory_bytes: u64,
    /// CPU utilization percentage
    pub cpu_utilization: f64,
    /// Tick processing time in milliseconds
    pub tick_processing_time_ms: TimeSeries<u64>,
    /// Memory allocations
    pub memory_allocations: u64,
    /// Garbage collection events
    pub gc_events: u64,
    /// Cache hit rates
    pub cache_hit_rates: HashMap<String, f64>,
}

/// Network simulation metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkMetrics {
    /// Messages sent
    pub messages_sent: u64,
    /// Messages received
    pub messages_received: u64,
    /// Messages dropped
    pub messages_dropped: u64,
    /// Network latency in milliseconds
    pub latency_ms: TimeSeries<u64>,
    /// Bandwidth utilization
    pub bandwidth_utilization: f64,
    /// Partition events
    pub partition_events: u64,
    /// Connection failures
    pub connection_failures: u64,
}

/// Protocol execution metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolMetrics {
    /// Active protocol sessions
    pub active_sessions: usize,
    /// Completed protocol sessions
    pub completed_sessions: u64,
    /// Failed protocol sessions
    pub failed_sessions: u64,
    /// Protocol execution time per type
    pub execution_time_by_type: HashMap<String, TimeSeries<u64>>,
    /// Session success rates by protocol type
    pub success_rates: HashMap<String, f64>,
    /// Byzantine attack events
    pub byzantine_events: u64,
}

impl Default for SimulationMetrics {
    fn default() -> Self {
        Self {
            simulation: SimulationCoreMetrics::default(),
            property_monitoring: PropertyMonitoringMetrics::default(),
            performance: PerformanceMetrics::default(),
            network: NetworkMetrics::default(),
            protocol: ProtocolMetrics::default(),
            custom: HashMap::new(),
        }
    }
}

impl Default for SimulationCoreMetrics {
    fn default() -> Self {
        Self {
            current_tick: 0,
            current_time: 0,
            total_duration_ms: 0,
            participant_count: 0,
            events_per_tick: TimeSeries::new(),
            state_changes: 0,
            checkpoints_created: 0,
            time_travel_operations: 0,
        }
    }
}

impl Default for PropertyMonitoringMetrics {
    fn default() -> Self {
        Self {
            total_evaluations: 0,
            evaluation_time_ms: TimeSeries::new(),
            violations_detected: 0,
            properties_per_evaluation: TimeSeries::new(),
            success_rate: 1.0,
            confidence_scores: TimeSeries::new(),
            trace_memory_bytes: 0,
        }
    }
}

impl Default for PerformanceMetrics {
    fn default() -> Self {
        Self {
            memory_usage_bytes: 0,
            peak_memory_bytes: 0,
            cpu_utilization: 0.0,
            tick_processing_time_ms: TimeSeries::new(),
            memory_allocations: 0,
            gc_events: 0,
            cache_hit_rates: HashMap::new(),
        }
    }
}

impl Default for NetworkMetrics {
    fn default() -> Self {
        Self {
            messages_sent: 0,
            messages_received: 0,
            messages_dropped: 0,
            latency_ms: TimeSeries::new(),
            bandwidth_utilization: 0.0,
            partition_events: 0,
            connection_failures: 0,
        }
    }
}

impl Default for ProtocolMetrics {
    fn default() -> Self {
        Self {
            active_sessions: 0,
            completed_sessions: 0,
            failed_sessions: 0,
            execution_time_by_type: HashMap::new(),
            success_rates: HashMap::new(),
            byzantine_events: 0,
        }
    }
}

impl SimulationMetrics {
    /// Create new metrics instance
    pub fn new() -> Self {
        Self::default()
    }

    /// Record simulation tick
    pub fn record_tick(&mut self, tick: u64, events_count: u64) {
        self.simulation.current_tick = tick;
        self.simulation
            .events_per_tick
            .add_point(tick, events_count);
    }

    /// Record property evaluation
    pub fn record_property_evaluation(
        &mut self,
        duration_ms: u64,
        properties_count: usize,
        violations: usize,
    ) {
        self.property_monitoring.total_evaluations += 1;
        self.property_monitoring.evaluation_time_ms.add_point(
            crate::utils::time::current_unix_timestamp_secs(),
            duration_ms,
        );
        self.property_monitoring
            .properties_per_evaluation
            .add_point(
                crate::utils::time::current_unix_timestamp_secs(),
                properties_count,
            );
        self.property_monitoring.violations_detected += violations as u64;

        // Update success rate
        let total_checks = self.property_monitoring.total_evaluations as f64;
        let successful_checks = total_checks - self.property_monitoring.violations_detected as f64;
        self.property_monitoring.success_rate = successful_checks / total_checks;
    }

    /// Record network message
    pub fn record_message(&mut self, sent: bool, dropped: bool, latency_ms: Option<u64>) {
        if sent {
            self.network.messages_sent += 1;
        } else {
            self.network.messages_received += 1;
        }

        if dropped {
            self.network.messages_dropped += 1;
        }

        if let Some(latency) = latency_ms {
            self.network
                .latency_ms
                .add_point(crate::utils::time::current_unix_timestamp_secs(), latency);
        }
    }

    /// Record protocol session completion
    pub fn record_protocol_completion(
        &mut self,
        protocol_type: &str,
        success: bool,
        duration_ms: u64,
    ) {
        if success {
            self.protocol.completed_sessions += 1;
        } else {
            self.protocol.failed_sessions += 1;
        }

        // Record execution time by type
        let timestamp = crate::utils::time::current_unix_timestamp_secs();
        self.protocol
            .execution_time_by_type
            .entry(protocol_type.to_string())
            .or_insert_with(TimeSeries::new)
            .add_point(timestamp, duration_ms);

        // Update success rate for this protocol type
        let total_for_type = self
            .protocol
            .execution_time_by_type
            .get(protocol_type)
            .map(|ts| ts.len())
            .unwrap_or(0) as f64;

        if total_for_type > 0.0 {
            let success_rate = self.protocol.completed_sessions as f64 / total_for_type;
            self.protocol
                .success_rates
                .insert(protocol_type.to_string(), success_rate);
        }
    }

    /// Record performance metrics
    pub fn record_performance(
        &mut self,
        memory_bytes: u64,
        cpu_utilization: f64,
        tick_time_ms: u64,
    ) {
        self.performance.memory_usage_bytes = memory_bytes;
        self.performance.peak_memory_bytes = self.performance.peak_memory_bytes.max(memory_bytes);
        self.performance.cpu_utilization = cpu_utilization;

        let timestamp = crate::utils::time::current_unix_timestamp_secs();
        self.performance
            .tick_processing_time_ms
            .add_point(timestamp, tick_time_ms);
    }

    /// Add custom metric
    pub fn add_custom_metric<S: Into<String>>(&mut self, name: S, value: MetricValue) {
        self.custom.insert(name.into(), value);
    }

    /// Get metrics summary
    pub fn summary(&self) -> MetricsSummary {
        MetricsSummary {
            total_ticks: self.simulation.current_tick,
            total_events: self.simulation.events_per_tick.total(),
            total_evaluations: self.property_monitoring.total_evaluations,
            violations_detected: self.property_monitoring.violations_detected,
            messages_sent: self.network.messages_sent,
            messages_dropped: self.network.messages_dropped,
            completed_sessions: self.protocol.completed_sessions,
            failed_sessions: self.protocol.failed_sessions,
            peak_memory_mb: self.performance.peak_memory_bytes / (1024 * 1024),
            average_cpu_utilization: self.performance.cpu_utilization,
        }
    }
}

/// Summary of key metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsSummary {
    pub total_ticks: u64,
    pub total_events: u64,
    pub total_evaluations: u64,
    pub violations_detected: u64,
    pub messages_sent: u64,
    pub messages_dropped: u64,
    pub completed_sessions: u64,
    pub failed_sessions: u64,
    pub peak_memory_mb: u64,
    pub average_cpu_utilization: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_creation() {
        let metrics = SimulationMetrics::new();
        assert_eq!(metrics.simulation.current_tick, 0);
        assert_eq!(metrics.property_monitoring.total_evaluations, 0);
        assert_eq!(metrics.network.messages_sent, 0);
    }

    #[test]
    fn test_tick_recording() {
        let mut metrics = SimulationMetrics::new();
        metrics.record_tick(1, 5);
        metrics.record_tick(2, 8);

        assert_eq!(metrics.simulation.current_tick, 2);
        assert_eq!(metrics.simulation.events_per_tick.len(), 2);
    }

    #[test]
    fn test_property_evaluation_recording() {
        let mut metrics = SimulationMetrics::new();
        metrics.record_property_evaluation(100, 3, 0);
        metrics.record_property_evaluation(150, 3, 1);

        assert_eq!(metrics.property_monitoring.total_evaluations, 2);
        assert_eq!(metrics.property_monitoring.violations_detected, 1);
        assert_eq!(metrics.property_monitoring.success_rate, 0.5);
    }

    #[test]
    fn test_message_recording() {
        let mut metrics = SimulationMetrics::new();
        metrics.record_message(true, false, Some(50));
        metrics.record_message(false, false, Some(75));
        metrics.record_message(true, true, None);

        assert_eq!(metrics.network.messages_sent, 2);
        assert_eq!(metrics.network.messages_received, 1);
        assert_eq!(metrics.network.messages_dropped, 1);
        assert_eq!(metrics.network.latency_ms.len(), 2);
    }

    #[test]
    fn test_protocol_completion_recording() {
        let mut metrics = SimulationMetrics::new();
        metrics.record_protocol_completion("dkg", true, 1000);
        metrics.record_protocol_completion("dkg", false, 500);
        metrics.record_protocol_completion("signing", true, 200);

        assert_eq!(metrics.protocol.completed_sessions, 2);
        assert_eq!(metrics.protocol.failed_sessions, 1);
        assert!(metrics.protocol.execution_time_by_type.contains_key("dkg"));
        assert!(metrics
            .protocol
            .execution_time_by_type
            .contains_key("signing"));
    }

    #[test]
    fn test_custom_metrics() {
        let mut metrics = SimulationMetrics::new();
        metrics.add_custom_metric("test_counter", MetricValue::Counter(42));
        metrics.add_custom_metric("test_gauge", MetricValue::Gauge(3.14));

        assert_eq!(metrics.custom.len(), 2);
        assert!(matches!(
            metrics.custom.get("test_counter"),
            Some(MetricValue::Counter(42))
        ));
    }

    #[test]
    fn test_metrics_summary() {
        let mut metrics = SimulationMetrics::new();
        metrics.record_tick(10, 100);
        metrics.record_property_evaluation(50, 2, 1);
        metrics.record_message(true, false, Some(25));
        metrics.record_protocol_completion("test", true, 500);

        let summary = metrics.summary();
        assert_eq!(summary.total_ticks, 10);
        assert_eq!(summary.violations_detected, 1);
        assert_eq!(summary.messages_sent, 1);
        assert_eq!(summary.completed_sessions, 1);
    }
}
