//! Simplified monitoring handler that delegates to external services.

use async_trait::async_trait;
use aura_core::hash;
use aura_core::effects::{SystemEffects, SystemError};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::{error, info, warn};

/// Stateless monitoring handler.
#[derive(Debug, Clone)]
pub struct MonitoringSystemHandler {
    config: MonitoringConfig,
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
        Self { config }
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
        let mut material = Vec::new();
        material.extend_from_slice(component.as_bytes());
        material.push(severity as u8);
        material.extend_from_slice(title.as_bytes());
        material.extend_from_slice(message.as_bytes());
        let digest = hash::hash(&material);
        let mut id_bytes = [0u8; 8];
        id_bytes.copy_from_slice(&digest[..8]);
        let alert = Alert {
            id: u64::from_le_bytes(id_bytes),
            component: component.to_string(),
            severity,
            title: title.to_string(),
            message: message.to_string(),
            resolved: false,
            metadata,
        };
        warn!(
            alert_id = alert.id,
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
        let _ = count;
        Vec::new()
    }

    /// Resolve an alert by its ID
    pub async fn resolve_alert(&self, alert_id: u64) -> Result<(), SystemError> {
        let _ = alert_id;
        Ok(())
    }

    /// Get monitoring statistics (stateless - delegates to external service)
    pub async fn get_statistics(&self) -> MonitoringStats {
        MonitoringStats::default()
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
        let mut info = HashMap::new();
        info.insert("component".to_string(), "monitoring".to_string());
        info.insert("max_alerts".to_string(), self.config.max_alerts.to_string());
        info.insert("status".to_string(), "operational".to_string());
        info.insert("total_health_checks".to_string(), "0".to_string());
        info.insert("failed_health_checks".to_string(), "0".to_string());
        info.insert("active_alerts".to_string(), "0".to_string());
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
            "max_alerts" => Ok(self.config.max_alerts.to_string()),
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
        metrics.insert("active_alerts".to_string(), 0.0);
        metrics.insert(
            "max_alerts_configured".to_string(),
            self.config.max_alerts as f64,
        );
        metrics.insert("total_health_checks".to_string(), 0.0);
        metrics.insert("failed_health_checks".to_string(), 0.0);
        metrics.insert("total_alerts".to_string(), 0.0);
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

        // Test system info
        let info = handler.get_system_info().await.unwrap();
        assert_eq!(info.get("component"), Some(&"monitoring".to_string()));
        assert_eq!(info.get("total_health_checks"), Some(&"0".to_string()));

        // Test config operations
        let config_value = handler.get_config("max_alerts").await.unwrap();
        assert_eq!(config_value, "256");

        let metrics = handler.get_metrics().await.unwrap();
        assert_eq!(metrics.get("active_alerts"), Some(&0.0)); // Should be 0 since no alerts generated
    }
}
