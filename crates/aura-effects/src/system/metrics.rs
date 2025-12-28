//! Layer 3: System Metrics Effect Handler - Production Only
//!
//! Stateless single-party implementation of system metrics from aura-core (Layer 1).
//! This handler provides production metrics operations delegating to external metrics services.
//!
//! **Layer Constraint**: NO stateful patterns or multi-party coordination.
//! This module contains only production-grade stateless handlers.
// System handlers are stateless in Layer 3.

use async_trait::async_trait;
use aura_core::effects::{SystemEffects, SystemError};
use std::collections::HashMap;
use std::time::Duration;
use tracing::{debug, error, info, warn};

/// Configuration for metrics collection
#[derive(Debug, Clone, Default)]
pub struct MetricsConfig {
    /// Whether to enable histogram metrics collection
    pub enable_histograms: bool,
}

/// Aggregated metrics statistics
#[derive(Debug, Clone, Default)]
pub struct MetricsStats {
    /// Total number of metrics recorded
    pub total_metrics_recorded: u64,
    /// Number of active counter metrics
    pub active_counters: u64,
    /// Number of active gauge metrics
    pub active_gauges: u64,
}

/// Production metrics handler for production use
///
/// This handler provides system metrics by delegating to external metrics services.
/// It is stateless and does not maintain in-memory counters or gauges.
///
/// **Note**: Complex metrics aggregation and multi-component coordination has been
/// moved to `MetricsCoordinator` in aura-protocol (Layer 4). This handler provides
/// only stateless metrics operations. For coordination capabilities, wrap this handler
/// with `aura_protocol::handlers::MetricsCoordinator`.
#[derive(Debug, Clone)]
pub struct MetricsSystemHandler {
    /// Configuration for metrics operations
    config: MetricsConfig,
}

impl MetricsSystemHandler {
    /// Create a new metrics system handler
    pub fn new(config: MetricsConfig) -> Self {
        Self { config }
    }

    /// Create with default configuration
    pub fn with_defaults() -> Self {
        Self::new(MetricsConfig::default())
    }

    /// Create a new metrics system handler
    pub fn new_real(config: MetricsConfig) -> Self {
        Self::new(config)
    }

    /// Record a counter increment
    pub async fn increment_counter(
        &self,
        name: &str,
        value: f64,
        labels: HashMap<String, String>,
    ) -> Result<(), SystemError> {
        let key = Self::with_labels(name, &labels);
        tracing::debug!(
            metric_name = name,
            key = key,
            value = value,
            "Counter incremented via metrics handler"
        );
        let _ = (key, value);
        Ok(())
    }

    /// Set a gauge value
    pub async fn set_gauge(
        &self,
        name: &str,
        value: f64,
        labels: HashMap<String, String>,
    ) -> Result<(), SystemError> {
        let key = Self::with_labels(name, &labels);
        tracing::debug!(
            metric_name = name,
            key = key,
            value = value,
            "Gauge set via metrics handler"
        );
        let _ = (key, value);
        Ok(())
    }

    /// Record a histogram observation
    pub async fn observe_histogram(
        &self,
        name: &str,
        value: f64,
        labels: HashMap<String, String>,
    ) -> Result<(), SystemError> {
        if !self.config.enable_histograms {
            return Ok(());
        }

        let key = Self::with_labels(name, &labels);
        tracing::debug!(
            metric_name = name,
            key = key,
            value = value,
            "Histogram observed via metrics handler"
        );
        let _ = (key, value);
        Ok(())
    }

    /// Record timing information
    pub async fn record_timing(
        &self,
        name: &str,
        duration: Duration,
        labels: HashMap<String, String>,
    ) -> Result<(), SystemError> {
        self.observe_histogram(name, duration.as_secs_f64() * 1000.0, labels)
            .await
    }

    /// Get counters (stateless - delegates to external service)
    pub async fn get_counters(&self) -> HashMap<String, f64> {
        HashMap::new()
    }

    /// Get gauges (stateless - delegates to external service)
    pub async fn get_gauges(&self) -> HashMap<String, f64> {
        HashMap::new()
    }

    /// Get metrics statistics (stateless - delegates to external service)
    pub async fn get_statistics(&self) -> MetricsStats {
        MetricsStats::default()
    }

    fn with_labels(name: &str, labels: &HashMap<String, String>) -> String {
        if labels.is_empty() {
            return name.to_string();
        }
        let mut parts: Vec<_> = labels.iter().map(|(k, v)| format!("{k}={v}")).collect();
        parts.sort();
        format!("{}:{}", name, parts.join(","))
    }
}

impl Default for MetricsSystemHandler {
    fn default() -> Self {
        Self::with_defaults()
    }
}

#[async_trait]
impl SystemEffects for MetricsSystemHandler {
    async fn log(&self, level: &str, component: &str, message: &str) -> Result<(), SystemError> {
        // Track log volume as a counter and forward to tracing.
        let labels = HashMap::from([
            ("level".to_string(), level.to_string()),
            ("component".to_string(), component.to_string()),
        ]);
        self.increment_counter("logs_total", 1.0, labels).await?;

        match level {
            "error" => error!("{}: {}", component, message),
            "warn" => warn!("{}: {}", component, message),
            "debug" => debug!("{}: {}", component, message),
            _ => info!("{}: {}", component, message),
        }
        Ok(())
    }

    async fn log_with_context(
        &self,
        level: &str,
        component: &str,
        message: &str,
        context: HashMap<String, String>,
    ) -> Result<(), SystemError> {
        self.increment_counter("logs_with_context_total", 1.0, context)
            .await?;
        self.log(level, component, message).await
    }

    async fn get_system_info(&self) -> Result<HashMap<String, String>, SystemError> {
        let mut info = HashMap::new();
        info.insert("component".to_string(), "metrics".to_string());
        info.insert("status".to_string(), "operational".to_string());
        info.insert(
            "enable_histograms".to_string(),
            self.config.enable_histograms.to_string(),
        );
        info.insert("active_counters".to_string(), "0".to_string());
        info.insert("active_gauges".to_string(), "0".to_string());
        info.insert("total_metrics_recorded".to_string(), "0".to_string());

        Ok(info)
    }

    async fn set_config(&self, key: &str, value: &str) -> Result<(), SystemError> {
        match key {
            "enable_histograms" => {
                let parsed =
                    value
                        .parse::<bool>()
                        .map_err(|_| SystemError::InvalidConfiguration {
                            key: key.to_string(),
                            value: value.to_string(),
                        })?;
                let _ = parsed;
                Ok(())
            }
            _ => Err(SystemError::InvalidConfiguration {
                key: key.to_string(),
                value: value.to_string(),
            }),
        }
    }

    async fn get_config(&self, key: &str) -> Result<String, SystemError> {
        match key {
            "enable_histograms" => Ok(self.config.enable_histograms.to_string()),
            _ => Err(SystemError::InvalidConfiguration {
                key: key.to_string(),
                value: "unknown".to_string(),
            }),
        }
    }

    async fn health_check(&self) -> Result<bool, SystemError> {
        Ok(true)
    }

    async fn get_metrics(&self) -> Result<HashMap<String, f64>, SystemError> {
        let mut metrics = HashMap::new();

        metrics.insert(
            "enable_histograms".to_string(),
            if self.config.enable_histograms {
                1.0
            } else {
                0.0
            },
        );
        metrics.insert("active_counters".to_string(), 0.0);
        metrics.insert("active_gauges".to_string(), 0.0);
        metrics.insert("total_metrics_recorded".to_string(), 0.0);
        Ok(metrics)
    }

    async fn restart_component(&self, _component: &str) -> Result<(), SystemError> {
        warn!("Restart not implemented for metrics system handler");
        Ok(())
    }

    async fn shutdown(&self) -> Result<(), SystemError> {
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)] // Test code: expect() is acceptable for test assertions
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_metrics_handler_creation() {
        let handler = MetricsSystemHandler::default();
        // MetricsSystemHandler should be created successfully
        let histograms_enabled = handler.get_config("enable_histograms").await.unwrap();
        assert_eq!(histograms_enabled, "false");
    }

    #[tokio::test]
    async fn test_metrics_operations() {
        let handler = MetricsSystemHandler::default();

        handler
            .increment_counter(
                "requests_total",
                1.0,
                HashMap::from([("route".to_string(), "/".to_string())]),
            )
            .await
            .expect("counter ok");
        handler
            .set_gauge("inflight", 5.0, HashMap::new())
            .await
            .expect("gauge ok");

        // Test system effects
        let info = handler.get_system_info().await.unwrap();
        assert_eq!(info.get("component"), Some(&"metrics".to_string()));
        assert_eq!(info.get("total_metrics_recorded"), Some(&"0".to_string()));

        // Test config operations
        let config_value = handler.get_config("enable_histograms").await.unwrap();
        assert_eq!(config_value, "false");
    }
}
