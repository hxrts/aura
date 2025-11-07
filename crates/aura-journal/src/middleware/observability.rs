//! Observability middleware for metrics and tracing

use super::{JournalContext, JournalHandler, JournalMiddleware};
use crate::error::Result;
use crate::operations::JournalOperation;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Instant;

/// Observability middleware that collects metrics and traces
pub struct ObservabilityMiddleware {
    /// Metrics collector
    metrics: Arc<MetricsCollector>,

    /// Configuration
    config: ObservabilityConfig,
}

impl ObservabilityMiddleware {
    /// Create new observability middleware
    pub fn new(config: ObservabilityConfig) -> Self {
        Self {
            metrics: Arc::new(MetricsCollector::new()),
            config,
        }
    }

    /// Get access to metrics
    pub fn metrics(&self) -> Arc<MetricsCollector> {
        self.metrics.clone()
    }
}

impl JournalMiddleware for ObservabilityMiddleware {
    // [VERIFIED] Uses Instant for metrics timing
    #[allow(clippy::disallowed_methods)]
    fn process(
        &self,
        operation: JournalOperation,
        context: &JournalContext,
        next: &dyn JournalHandler,
    ) -> Result<serde_json::Value> {
        let start_time = Instant::now();
        let operation_name = format!("{:?}", operation);

        // Record operation start
        if self.config.collect_operation_metrics {
            self.metrics.increment_counter(
                "journal_operations_total",
                &[
                    ("operation", &operation_name),
                    ("account_id", &context.account_id.to_string()),
                ],
            );
        }

        // Execute operation
        let result = next.handle(operation, context);

        let duration = start_time.elapsed();

        // Record operation completion
        match &result {
            Ok(_) => {
                if self.config.collect_operation_metrics {
                    self.metrics.increment_counter(
                        "journal_operations_success_total",
                        &[
                            ("operation", &operation_name),
                            ("account_id", &context.account_id.to_string()),
                        ],
                    );
                }

                if self.config.collect_timing_metrics {
                    self.metrics.record_histogram(
                        "journal_operation_duration_seconds",
                        duration.as_secs_f64(),
                        &[("operation", &operation_name)],
                    );
                }
            }
            Err(error) => {
                if self.config.collect_error_metrics {
                    self.metrics.increment_counter(
                        "journal_operations_error_total",
                        &[
                            ("operation", &operation_name),
                            ("error_type", &error.to_string()),
                            ("account_id", &context.account_id.to_string()),
                        ],
                    );
                }
            }
        }

        result
    }

    fn name(&self) -> &str {
        "observability"
    }
}

/// Configuration for observability middleware
#[derive(Debug, Clone)]
pub struct ObservabilityConfig {
    /// Whether to collect operation metrics
    pub collect_operation_metrics: bool,

    /// Whether to collect timing metrics
    pub collect_timing_metrics: bool,

    /// Whether to collect error metrics
    pub collect_error_metrics: bool,

    /// Whether to trace operations
    pub enable_tracing: bool,

    /// Sampling rate for traces (0.0 to 1.0)
    pub trace_sampling_rate: f64,
}

impl Default for ObservabilityConfig {
    fn default() -> Self {
        Self {
            collect_operation_metrics: true,
            collect_timing_metrics: true,
            collect_error_metrics: true,
            enable_tracing: false,
            trace_sampling_rate: 0.1,
        }
    }
}

/// Simple metrics collector
pub struct MetricsCollector {
    /// Counter metrics
    counters: RwLock<HashMap<String, AtomicU64>>,

    /// Histogram metrics (simplified as just count and sum)
    histograms: RwLock<HashMap<String, HistogramData>>,
}

#[derive(Debug)]
struct HistogramData {
    count: AtomicU64,
    sum: RwLock<f64>,
}

impl MetricsCollector {
    /// Create a new metrics collector
    pub fn new() -> Self {
        Self {
            counters: RwLock::new(HashMap::new()),
            histograms: RwLock::new(HashMap::new()),
        }
    }

    /// Increment a counter metric
    pub fn increment_counter(&self, name: &str, labels: &[(&str, &str)]) {
        let key = Self::metric_key(name, labels);
        let counters = self.counters.read().unwrap();

        if let Some(counter) = counters.get(&key) {
            counter.fetch_add(1, Ordering::Relaxed);
        } else {
            drop(counters);
            let mut counters = self.counters.write().unwrap();
            counters
                .entry(key)
                .or_insert_with(|| AtomicU64::new(0))
                .fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Record a histogram value
    pub fn record_histogram(&self, name: &str, value: f64, labels: &[(&str, &str)]) {
        let key = Self::metric_key(name, labels);
        let histograms = self.histograms.read().unwrap();

        if let Some(histogram) = histograms.get(&key) {
            histogram.count.fetch_add(1, Ordering::Relaxed);
            *histogram.sum.write().unwrap() += value;
        } else {
            drop(histograms);
            let mut histograms = self.histograms.write().unwrap();
            histograms.insert(
                key,
                HistogramData {
                    count: AtomicU64::new(1),
                    sum: RwLock::new(value),
                },
            );
        }
    }

    /// Get counter value
    pub fn get_counter(&self, name: &str, labels: &[(&str, &str)]) -> u64 {
        let key = Self::metric_key(name, labels);
        self.counters
            .read()
            .unwrap()
            .get(&key)
            .map(|c| c.load(Ordering::Relaxed))
            .unwrap_or(0)
    }

    /// Get histogram stats
    pub fn get_histogram_stats(&self, name: &str, labels: &[(&str, &str)]) -> Option<(u64, f64)> {
        let key = Self::metric_key(name, labels);
        self.histograms
            .read()
            .unwrap()
            .get(&key)
            .map(|h| (h.count.load(Ordering::Relaxed), *h.sum.read().unwrap()))
    }

    /// Reset all metrics
    pub fn reset(&self) {
        self.counters.write().unwrap().clear();
        self.histograms.write().unwrap().clear();
    }

    fn metric_key(name: &str, labels: &[(&str, &str)]) -> String {
        if labels.is_empty() {
            name.to_string()
        } else {
            let label_str = labels
                .iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect::<Vec<_>>()
                .join(",");
            format!("{}{{{}}}", name, label_str)
        }
    }
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::middleware::handler::NoOpHandler;
    use crate::operations::JournalOperation;
    use aura_crypto::Effects;
    use aura_types::{AccountIdExt, DeviceIdExt};

    #[test]
    fn test_observability_middleware() {
        let effects = Effects::test(42);
        let account_id = aura_types::AccountId::new_with_effects(&effects);
        let device_id = aura_types::DeviceId::new_with_effects(&effects);

        let middleware = ObservabilityMiddleware::new(ObservabilityConfig::default());
        let handler = NoOpHandler;
        let context = JournalContext::new(account_id, device_id, "test".to_string());
        let operation = JournalOperation::GetEpoch;

        // Process operation
        let result = middleware.process(operation, &context, &handler);
        assert!(result.is_ok());

        // Check metrics were recorded
        let metrics = middleware.metrics();
        let operation_count = metrics.get_counter(
            "journal_operations_total",
            &[
                ("operation", "GetEpoch"),
                ("account_id", &account_id.to_string()),
            ],
        );
        assert_eq!(operation_count, 1);

        let success_count = metrics.get_counter(
            "journal_operations_success_total",
            &[
                ("operation", "GetEpoch"),
                ("account_id", &account_id.to_string()),
            ],
        );
        assert_eq!(success_count, 1);
    }
}
