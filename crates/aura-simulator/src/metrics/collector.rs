//! Metrics collection implementation

use super::*;
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Metrics collector implementation
#[derive(Debug, Clone)]
pub struct MetricsCollector {
    /// Internal metrics storage
    metrics: Arc<Mutex<SimulationMetrics>>,
    /// Collection enabled flag
    enabled: bool,
}

/// Snapshot of metrics at a point in time
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsSnapshot {
    /// Timestamp when snapshot was taken
    pub timestamp: u64,
    /// Snapshot of simulation metrics
    pub metrics: SimulationMetrics,
    /// Metadata about the snapshot
    pub metadata: HashMap<String, String>,
}

impl MetricsSnapshot {
    /// Get a counter value by name
    pub fn get_counter(&self, name: &str) -> Option<u64> {
        self.metrics
            .custom
            .get(name)
            .and_then(|metric| match metric {
                MetricValue::Counter(value) => Some(*value),
                _ => None,
            })
    }
}

/// Timer guard that records duration when dropped
pub struct TimerGuard {
    name: String,
    start_time: Instant,
    collector: MetricsCollector,
}

impl TimerGuard {
    #[allow(clippy::disallowed_methods)]
    fn new(name: String, collector: MetricsCollector) -> Self {
        Self {
            name,
            start_time: Instant::now(),
            collector,
        }
    }
}

impl Drop for TimerGuard {
    fn drop(&mut self) {
        let duration_ms = self.start_time.elapsed().as_millis() as u64;
        self.collector.timer(&self.name, duration_ms);
    }
}

impl MetricsCollector {
    /// Create a new metrics collector
    pub fn new() -> Self {
        Self {
            metrics: Arc::new(Mutex::new(SimulationMetrics::new())),
            enabled: true,
        }
    }

    /// Create a disabled metrics collector (no-op)
    pub fn disabled() -> Self {
        Self {
            metrics: Arc::new(Mutex::new(SimulationMetrics::new())),
            enabled: false,
        }
    }

    /// Enable or disable metrics collection
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Check if metrics collection is enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Get a snapshot of current metrics
    pub fn snapshot(&self) -> MetricsSnapshot {
        // A poisoned mutex indicates a panic during updates, which is unrecoverable
        #[allow(clippy::expect_used)]
        let metrics = self
            .metrics
            .lock()
            .expect("Metrics mutex should not be poisoned")
            .clone();
        let mut metadata = HashMap::new();
        metadata.insert("collector_enabled".to_string(), self.enabled.to_string());
        metadata.insert(
            "snapshot_time".to_string(),
            crate::utils::time::current_unix_timestamp_secs().to_string(),
        );

        MetricsSnapshot {
            timestamp: crate::utils::time::current_unix_timestamp_secs(),
            metrics,
            metadata,
        }
    }

    /// Reset all metrics
    pub fn reset(&self) {
        if self.enabled {
            // A poisoned mutex indicates a panic during updates, which is unrecoverable
            #[allow(clippy::expect_used)]
            let mut metrics = self
                .metrics
                .lock()
                .expect("Metrics mutex should not be poisoned");
            *metrics = SimulationMetrics::new();
        }
    }

    /// Get current metrics summary
    pub fn summary(&self) -> MetricsSummary {
        // A poisoned mutex indicates a panic during updates, which is unrecoverable
        #[allow(clippy::expect_used)]
        self.metrics
            .lock()
            .expect("Metrics mutex should not be poisoned")
            .summary()
    }

    /// Execute a closure with mutable access to metrics (if enabled)
    pub fn with_metrics<F, R>(&self, f: F) -> Option<R>
    where
        F: FnOnce(&mut SimulationMetrics) -> R,
    {
        if self.enabled {
            // A poisoned mutex indicates a panic during updates, which is unrecoverable
            #[allow(clippy::expect_used)]
            let mut metrics = self
                .metrics
                .lock()
                .expect("Metrics mutex should not be poisoned");
            Some(f(&mut metrics))
        } else {
            None
        }
    }

    /// Start a timer that will record duration when dropped
    pub fn timer_start(&self, name: &str) -> TimerGuard {
        TimerGuard::new(name.to_string(), self.clone())
    }
}

impl MetricsProvider for MetricsCollector {
    fn counter(&self, name: &str, value: u64) {
        if !self.enabled {
            return;
        }

        self.with_metrics(|metrics| {
            metrics.add_custom_metric(name, MetricValue::Counter(value));
        });
    }

    fn gauge(&self, name: &str, value: f64) {
        if !self.enabled {
            return;
        }

        self.with_metrics(|metrics| {
            metrics.add_custom_metric(name, MetricValue::Gauge(value));
        });
    }

    fn histogram(&self, name: &str, value: f64) {
        if !self.enabled {
            return;
        }

        self.with_metrics(|metrics| {
            // For histogram, we'll append to existing values or create new
            if let Some(MetricValue::Histogram(ref mut values)) = metrics.custom.get_mut(name) {
                values.push(value);
            } else {
                // Create new histogram with single value
                metrics.add_custom_metric(name.to_string(), MetricValue::Histogram(vec![value]));
            }
        });
    }

    fn timer(&self, name: &str, duration_ms: u64) {
        if !self.enabled {
            return;
        }

        self.with_metrics(|metrics| {
            metrics.add_custom_metric(name, MetricValue::Timer(duration_ms));
        });
    }

    fn custom(&self, name: &str, value: MetricValue, metadata: HashMap<String, String>) {
        if !self.enabled {
            return;
        }

        self.with_metrics(|metrics| {
            metrics.add_custom_metric(name.to_string(), value);
            // Store metadata as additional custom metrics
            for (k, _v) in metadata {
                let meta_key = format!("{}_meta_{}", name, k);
                metrics.add_custom_metric(meta_key, MetricValue::Gauge(0.0)); // Placeholder for metadata
            }
        });
    }
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

/// Helper trait for metrics-aware components
pub trait WithMetrics {
    /// Get the metrics collector
    fn metrics(&self) -> &MetricsCollector;

    /// Record a performance measurement
    fn record_performance(&self, operation: &str, duration_ms: u64) {
        let metric_name = format!("performance_{}", operation);
        self.metrics().timer(&metric_name, duration_ms);
    }

    /// Record an event count
    fn record_event(&self, event_type: &str, count: u64) {
        let metric_name = format!("events_{}", event_type);
        self.metrics().counter(&metric_name, count);
    }

    /// Record a resource measurement
    fn record_resource(&self, resource: &str, value: f64) {
        let metric_name = format!("resource_{}", resource);
        self.metrics().gauge(&metric_name, value);
    }
}

/// Macro for easy metrics recording
#[macro_export]
macro_rules! record_metric {
    ($collector:expr, counter, $name:expr, $value:expr) => {
        $collector.counter($name, $value);
    };
    ($collector:expr, gauge, $name:expr, $value:expr) => {
        $collector.gauge($name, $value);
    };
    ($collector:expr, histogram, $name:expr, $value:expr) => {
        $collector.histogram($name, $value);
    };
    ($collector:expr, timer, $name:expr, $value:expr) => {
        $collector.timer($name, $value);
    };
}

/// Macro for timing code blocks
#[macro_export]
macro_rules! time_block {
    ($collector:expr, $name:expr, $block:block) => {{
        let _timer = $collector.timer_start($name);
        $block
    }};
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_collector_basic() {
        let collector = MetricsCollector::new();
        assert!(collector.is_enabled());

        collector.counter("test_counter", 42);
        collector.gauge("test_gauge", 3.14);
        collector.timer("test_timer", 1000);

        let snapshot = collector.snapshot();
        assert!(snapshot.metrics.custom.contains_key("test_counter"));
        assert!(snapshot.metrics.custom.contains_key("test_gauge"));
        assert!(snapshot.metrics.custom.contains_key("test_timer"));
    }

    #[test]
    fn test_metrics_collector_disabled() {
        let collector = MetricsCollector::disabled();
        assert!(!collector.is_enabled());

        collector.counter("test_counter", 42);

        let snapshot = collector.snapshot();
        assert!(!snapshot.metrics.custom.contains_key("test_counter"));
    }

    #[test]
    fn test_metrics_collector_histogram() {
        let collector = MetricsCollector::new();

        collector.histogram("response_time", 100.0);
        collector.histogram("response_time", 200.0);
        collector.histogram("response_time", 150.0);

        let snapshot = collector.snapshot();
        if let Some(MetricValue::Histogram(values)) = snapshot.metrics.custom.get("response_time") {
            assert_eq!(values.len(), 3);
            assert!(values.contains(&100.0));
            assert!(values.contains(&200.0));
            assert!(values.contains(&150.0));
        } else {
            panic!("Expected histogram metric");
        }
    }

    #[test]
    fn test_metrics_collector_timer_guard() {
        let collector = MetricsCollector::new();

        {
            let _timer = collector.timer_start("test_operation");
            std::thread::sleep(std::time::Duration::from_millis(10));
        } // Timer should record duration here

        let snapshot = collector.snapshot();
        assert!(snapshot.metrics.custom.contains_key("test_operation"));
    }

    #[test]
    fn test_metrics_collector_reset() {
        let collector = MetricsCollector::new();

        collector.counter("test_counter", 42);
        assert!(!collector.snapshot().metrics.custom.is_empty());

        collector.reset();
        assert!(collector.snapshot().metrics.custom.is_empty());
    }

    #[test]
    fn test_metrics_collector_summary() {
        let collector = MetricsCollector::new();

        collector.with_metrics(|metrics| {
            metrics.record_tick(10, 100);
            metrics.record_property_evaluation(50, 3, 1);
            metrics.record_message(true, false, Some(25));
        });

        let summary = collector.summary();
        assert_eq!(summary.total_ticks, 10);
        assert_eq!(summary.violations_detected, 1);
        assert_eq!(summary.messages_sent, 1);
    }

    #[test]
    fn test_record_metric_macro() {
        let collector = MetricsCollector::new();

        record_metric!(collector, counter, "macro_counter", 123);
        record_metric!(collector, gauge, "macro_gauge", 4.56);
        record_metric!(collector, timer, "macro_timer", 500);

        let snapshot = collector.snapshot();
        assert!(snapshot.metrics.custom.contains_key("macro_counter"));
        assert!(snapshot.metrics.custom.contains_key("macro_gauge"));
        assert!(snapshot.metrics.custom.contains_key("macro_timer"));
    }

    #[test]
    fn test_time_block_macro() {
        let collector = MetricsCollector::new();

        let result = time_block!(collector, "block_timer", {
            std::thread::sleep(std::time::Duration::from_millis(10));
            42
        });

        assert_eq!(result, 42);
        let snapshot = collector.snapshot();
        assert!(snapshot.metrics.custom.contains_key("block_timer"));
    }
}
