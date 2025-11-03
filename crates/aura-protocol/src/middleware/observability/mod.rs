//! Observability middleware
//!
//! Provides tracing, metrics, and logging decorators for effect handlers.

pub mod metrics;
pub mod tracing;

pub use metrics::MetricsMiddleware;
pub use tracing::TracingMiddleware;

/// Configuration for observability middleware
#[derive(Debug, Clone)]
pub struct ObservabilityConfig {
    /// Enable distributed tracing
    pub enable_tracing: bool,
    /// Enable metrics collection
    pub enable_metrics: bool,
    /// Sample rate for tracing (0.0 to 1.0)
    pub trace_sample_rate: f64,
    /// Metric collection interval in seconds
    pub metric_interval_secs: u64,
}

impl Default for ObservabilityConfig {
    fn default() -> Self {
        Self {
            enable_tracing: true,
            enable_metrics: true,
            trace_sample_rate: 1.0,
            metric_interval_secs: 60,
        }
    }
}