//! Simplified monitoring handler that keeps local alert state only.
// Uses std sync primitives for lightweight in-process state.
#![allow(clippy::disallowed_types)]

use async_trait::async_trait;
use aura_core::effects::{SystemEffects, SystemError};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use tracing::{error, info, warn};

/// In-memory monitoring handler state
#[derive(Debug, Clone)]
pub struct MonitoringSystemHandler {
    config: std::sync::Arc<std::sync::Mutex<MonitoringConfig>>,
    alerts: std::sync::Arc<std::sync::Mutex<Vec<Alert>>>,
    stats: std::sync::Arc<std::sync::Mutex<MonitoringStats>>,
    alert_counter: std::sync::Arc<AtomicU64>,
}

/// Health status levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HealthStatus {
    /// Component is operating normally
    Healthy,
    /// Component is operating with reduced performance or minor issues
    Degraded,
    /// Component has encountered significant problems
    Unhealthy,
    /// Component has encountered severe problems requiring immediate attention
    Critical,
}

/// Alert severity levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AlertSeverity {
    /// Informational alert, no action required
    Info,
    /// Warning alert, may require attention or investigation
    Warning,
    /// Critical alert, requires immediate attention
    Critical,
}

/// Health check result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheckResult {
    /// Name of the component being checked
    pub component: String,
    /// Current health status of the component
    pub status: HealthStatus,
    /// Status message or diagnostic information
    pub message: String,
}

/// Alert notification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alert {
    /// Unique alert identifier (deterministic counter-based for Layer 3 handlers)
    pub id: u64,
    /// Component that triggered the alert
    pub component: String,
    /// Alert severity level
    pub severity: AlertSeverity,
    /// Alert title/summary
    pub title: String,
    /// Detailed alert message
    pub message: String,
    /// Whether the alert has been resolved
    pub resolved: bool,
    /// Additional metadata for the alert
    pub metadata: HashMap<String, String>,
}

/// Configuration for monitoring system
#[derive(Debug, Clone)]
pub struct MonitoringConfig {
    /// Maximum number of alerts to keep in buffer
    pub max_alerts: usize,
}

impl Default for MonitoringConfig {
    fn default() -> Self {
        Self { max_alerts: 256 }
    }
}

/// Monitoring system statistics
#[derive(Debug, Clone, Default)]
pub struct MonitoringStats {
    /// Total number of health checks performed
    pub total_health_checks: u64,
    /// Number of failed health checks
    pub failed_health_checks: u64,
    /// Total number of alerts generated
    pub total_alerts: u64,
    /// Number of currently active (unresolved) alerts
    pub active_alerts: u64,
}

impl MonitoringSystemHandler {
    /// Create a new monitoring system handler
    pub fn new(config: MonitoringConfig) -> Self {
        Self {
            config: std::sync::Arc::new(std::sync::Mutex::new(config)),
            alerts: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            stats: std::sync::Arc::new(std::sync::Mutex::new(MonitoringStats::default())),
            alert_counter: std::sync::Arc::new(AtomicU64::new(1)),
        }
    }

    /// Create a monitoring system handler with default configuration
    pub fn with_defaults() -> Self {
        Self::new(MonitoringConfig::default())
    }

    /// Manually trigger a health check for a specific component
    pub async fn check_component_health(
        &self,
        component: &str,
    ) -> Result<HealthCheckResult, SystemError> {
        if let Ok(mut stats) = self.stats.lock() {
            stats.total_health_checks = stats.total_health_checks.saturating_add(1);
        }
        Ok(HealthCheckResult {
            component: component.to_string(),
            status: HealthStatus::Healthy,
            message: "ok".to_string(),
        })
    }

    /// Send a custom alert
    pub async fn send_alert(
        &self,
        component: &str,
        severity: AlertSeverity,
        title: &str,
        message: &str,
        metadata: HashMap<String, String>,
    ) -> Result<(), SystemError> {
        let alert = Alert {
            id: self.alert_counter.fetch_add(1, Ordering::SeqCst),
            component: component.to_string(),
            severity,
            title: title.to_string(),
            message: message.to_string(),
            resolved: false,
            metadata,
        };
        if let Ok(mut alerts) = self.alerts.lock() {
            alerts.push(alert.clone());
            let target = self
                .config
                .lock()
                .map(|c| c.max_alerts)
                .unwrap_or(MonitoringConfig::default().max_alerts);
            if alerts.len() > target {
                let overflow = alerts.len() - target;
                alerts.drain(0..overflow);
            }
        }
        if let Ok(mut stats) = self.stats.lock() {
            stats.total_alerts = stats.total_alerts.saturating_add(1);
            stats.active_alerts = self
                .alerts
                .lock()
                .map(|a| a.iter().filter(|a| !a.resolved).count() as u64)
                .unwrap_or(stats.active_alerts);
        }
        warn!(
            component,
            ?severity,
            title,
            message,
            "Monitoring alert emitted"
        );
        Ok(())
    }

    /// Get the most recent alerts up to the specified count
    pub async fn get_recent_alerts(&self, count: usize) -> Vec<Alert> {
        let alerts = self.alerts.lock().map(|a| a.clone()).unwrap_or_default();
        let len = alerts.len();
        let start = len.saturating_sub(count);
        alerts[start..].to_vec()
    }

    /// Resolve an alert by its ID
    pub async fn resolve_alert(&self, alert_id: u64) -> Result<(), SystemError> {
        if let Ok(mut alerts) = self.alerts.lock() {
            for alert in alerts.iter_mut() {
                if alert.id == alert_id {
                    alert.resolved = true;
                }
            }
        }
        if let Ok(mut stats) = self.stats.lock() {
            stats.active_alerts = self
                .alerts
                .lock()
                .map(|a| a.iter().filter(|a| !a.resolved).count() as u64)
                .unwrap_or(stats.active_alerts);
        }
        Ok(())
    }

    /// Get monitoring statistics (stateless - delegates to external service)
    pub async fn get_statistics(&self) -> MonitoringStats {
        self.stats.lock().map(|s| s.clone()).unwrap_or_default()
    }
}

impl Default for MonitoringSystemHandler {
    fn default() -> Self {
        Self::with_defaults()
    }
}

#[async_trait]
impl SystemEffects for MonitoringSystemHandler {
    async fn log(&self, level: &str, component: &str, message: &str) -> Result<(), SystemError> {
        match level {
            "error" => error!("{}: {}", component, message),
            "warn" => warn!("{}: {}", component, message),
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
        let context_str = context
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<_>>()
            .join(", ");
        let full_message = format!("{} [{}]", message, context_str);
        self.log(level, component, &full_message).await
    }

    async fn get_system_info(&self) -> Result<HashMap<String, String>, SystemError> {
        let stats = self.stats.lock().map(|s| s.clone()).unwrap_or_default();
        let config = self
            .config
            .lock()
            .map(|c| c.clone())
            .unwrap_or_else(|_| MonitoringConfig::default());
        let mut info = HashMap::new();
        info.insert("component".to_string(), "monitoring".to_string());
        info.insert("max_alerts".to_string(), config.max_alerts.to_string());
        info.insert("status".to_string(), "operational".to_string());
        info.insert(
            "total_health_checks".to_string(),
            stats.total_health_checks.to_string(),
        );
        info.insert(
            "failed_health_checks".to_string(),
            stats.failed_health_checks.to_string(),
        );
        info.insert("active_alerts".to_string(), stats.active_alerts.to_string());
        Ok(info)
    }

    async fn set_config(&self, key: &str, value: &str) -> Result<(), SystemError> {
        match key {
            "max_alerts" => {
                let parsed =
                    value
                        .parse::<usize>()
                        .map_err(|_| SystemError::InvalidConfiguration {
                            key: key.to_string(),
                            value: value.to_string(),
                        })?;
                if let Ok(mut config) = self.config.lock() {
                    config.max_alerts = parsed;
                }
                Ok(())
            }
            _ => Err(SystemError::InvalidConfiguration {
                key: key.to_string(),
                value: value.to_string(),
            }),
        }
    }

    async fn get_config(&self, key: &str) -> Result<String, SystemError> {
        let config = self
            .config
            .lock()
            .map(|c| c.clone())
            .unwrap_or_else(|_| MonitoringConfig::default());
        match key {
            "max_alerts" => Ok(config.max_alerts.to_string()),
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
        let stats = self.stats.lock().map(|s| s.clone()).unwrap_or_default();
        let config = self
            .config
            .lock()
            .map(|c| c.clone())
            .unwrap_or_else(|_| MonitoringConfig::default());
        let mut metrics = HashMap::new();
        metrics.insert("active_alerts".to_string(), stats.active_alerts as f64);
        metrics.insert(
            "max_alerts_configured".to_string(),
            config.max_alerts as f64,
        );
        metrics.insert(
            "total_health_checks".to_string(),
            stats.total_health_checks as f64,
        );
        metrics.insert(
            "failed_health_checks".to_string(),
            stats.failed_health_checks as f64,
        );
        metrics.insert("total_alerts".to_string(), stats.total_alerts as f64);
        Ok(metrics)
    }

    async fn restart_component(&self, component: &str) -> Result<(), SystemError> {
        tracing::warn!(
            component = component,
            "Restart requested via monitoring handler"
        );
        Err(SystemError::OperationFailed {
            message: "restart_component not supported in monitoring handler".to_string(),
        })
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
    async fn test_monitoring_handler_creation() {
        let handler = MonitoringSystemHandler::default();
        // MonitoringSystemHandler should be created successfully
        let max_alerts = handler.get_config("max_alerts").await.unwrap();
        assert_eq!(max_alerts, "256");
    }

    #[tokio::test]
    async fn test_alert_operations() {
        let handler = MonitoringSystemHandler::default();

        handler
            .send_alert(
                "component",
                AlertSeverity::Warning,
                "title",
                "body",
                HashMap::new(),
            )
            .await
            .expect("alert ok");

        // Test health check
        let health = handler
            .check_component_health("test_component")
            .await
            .unwrap();
        assert_eq!(health.component, "test_component");
        assert_eq!(health.status, HealthStatus::Healthy);
    }

    #[tokio::test]
    async fn test_system_effects() {
        let handler = MonitoringSystemHandler::default();

        // Perform a health check first to increment the counter
        let _health = handler
            .check_component_health("test_component")
            .await
            .unwrap();

        // Test system info
        let info = handler.get_system_info().await.unwrap();
        assert_eq!(info.get("component"), Some(&"monitoring".to_string()));
        assert_eq!(info.get("total_health_checks"), Some(&"1".to_string()));

        // Test config operations
        let config_value = handler.get_config("max_alerts").await.unwrap();
        assert_eq!(config_value, "256");

        let metrics = handler.get_metrics().await.unwrap();
        assert_eq!(metrics.get("active_alerts"), Some(&0.0)); // Should be 0 since no alerts generated
    }
}
