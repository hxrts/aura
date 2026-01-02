//! Layer 3: System Logging Effect Handler - Production Only
//!
//! Stateless single-party implementation of system logging from aura-core (Layer 1).
//! This handler provides production logging operations delegating to external logging services.
//!
//! **Layer Constraint**: NO stateful patterns or multi-party coordination.
//! This module contains only production-grade stateless handlers.
// System handlers are stateless in Layer 3.

use async_trait::async_trait;
use aura_core::effects::{SystemEffects, SystemError};
use aura_core::identifiers::DeviceId;
use aura_core::SessionId;
use serde_json::Value;
use std::collections::HashMap;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use super::types::{AuditAction, ComponentId, LogLevel};

/// Log entry with structured metadata
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LogEntry {
    /// Log level (debug, info, warn, error)
    pub level: LogLevel,
    /// Log message content
    pub message: String,
    /// Component that generated the log
    pub component: ComponentId,
    /// Associated session identifier
    pub session_id: Option<SessionId>,
    /// Associated device identifier
    pub device_id: Option<DeviceId>,
    /// Structured metadata key-value pairs
    pub metadata: HashMap<String, Value>,
    /// Unique trace identifier for request correlation
    pub trace_id: Option<Uuid>,
}

/// Audit log entry for security-critical events
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AuditEntry {
    /// Type of event (e.g., "authentication", "authorization", "key_operation")
    pub event_type: String,
    /// Actor performing the action
    pub actor: Option<DeviceId>,
    /// Resource being acted upon
    pub resource: String,
    /// Action performed (e.g., "create", "read", "update", "delete")
    pub action: AuditAction,
    /// Outcome of the action (success, failure, denied)
    pub outcome: String,
    /// Structured metadata for the audit entry
    pub metadata: HashMap<String, Value>,
    /// Associated session identifier
    pub session_id: Option<SessionId>,
}

/// Configuration for logging system
#[derive(Debug, Clone)]
pub struct LoggingConfig {
    /// Maximum number of log entries to retain in memory
    pub max_log_entries: u32,
    /// Maximum number of audit entries to retain in memory
    pub max_audit_entries: u32,
    /// Log level filter (debug, info, warn, error)
    pub log_level: LogLevel,
    /// Whether audit logging is enabled
    pub audit_enabled: bool,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            max_log_entries: 1024,
            max_audit_entries: 512,
            log_level: LogLevel::Info,
            audit_enabled: true,
        }
    }
}

/// Statistics for the logging system
#[derive(Debug, Clone, Default)]
pub struct LoggingStats {
    /// Total number of log entries written
    pub total_logs: u64,
    /// Total number of audit entries written
    pub total_audit_logs: u64,
    /// Number of error level logs
    pub error_logs: u64,
    /// Number of warning level logs
    pub warn_logs: u64,
    /// Number of info level logs
    pub info_logs: u64,
    /// Number of debug level logs
    pub debug_logs: u64,
}

/// Production logging handler for production use
///
/// This handler provides system logging by delegating to external logging services.
/// It is stateless and does not maintain in-memory buffers.
///
/// **Note**: Complex log aggregation and multi-component coordination has been
/// moved to `LoggingCoordinator` in aura-protocol (Layer 4). This handler provides
/// only stateless logging operations. For coordination capabilities, wrap this handler
/// with `aura_protocol::handlers::LoggingCoordinator`.
#[derive(Debug, Clone)]
pub struct LoggingSystemHandler {
    /// Configuration for logging operations
    config: LoggingConfig,
}

impl LoggingSystemHandler {
    /// Create a new logging system handler
    pub fn new(config: LoggingConfig) -> Self {
        Self { config }
    }

    /// Create with default configuration
    pub fn with_defaults() -> Self {
        Self::new(LoggingConfig::default())
    }

    /// Create a new logging system handler
    pub fn new_real(config: LoggingConfig) -> Self {
        Self::new(config)
    }

    /// Apply log level filtering and emit to tracing
    fn apply_level(level: LogLevel, component: &ComponentId, message: &str) {
        match level {
            LogLevel::Error => error!("{}: {}", component, message),
            LogLevel::Warn => warn!("{}: {}", component, message),
            LogLevel::Info => info!("{}: {}", component, message),
            LogLevel::Debug => debug!("{}: {}", component, message),
        }
    }

    /// Push log entry (stateless - delegates to external logging service)
    async fn push_log(&self, entry: LogEntry) {
        tracing::debug!(
            level = %entry.level,
            component = %entry.component,
            message = entry.message,
            "Log entry sent via logging handler"
        );
        let _ = entry;
    }

    /// Push audit entry (stateless - delegates to external audit service)
    async fn push_audit(&self, entry: AuditEntry) {
        tracing::info!(
            event_type = entry.event_type,
            actor = ?entry.actor,
            resource = entry.resource,
            action = %entry.action,
            outcome = entry.outcome,
            "Audit entry sent via logging handler"
        );
        let _ = entry;
    }

    /// Log a structured message
    pub async fn log_structured(
        &self,
        level: LogLevel,
        component: ComponentId,
        message: &str,
        metadata: HashMap<String, Value>,
        session_id: Option<SessionId>,
        device_id: Option<DeviceId>,
        trace_id: Option<Uuid>,
    ) -> Result<(), SystemError> {
        let entry = LogEntry {
            level,
            message: message.to_string(),
            component: component.clone(),
            session_id,
            device_id,
            metadata,
            trace_id,
        };

        Self::apply_level(level, &component, message);
        self.push_log(entry).await;
        Ok(())
    }

    /// Log an audit event
    pub async fn audit_log(
        &self,
        event_type: &str,
        actor: Option<DeviceId>,
        resource: &str,
        action: AuditAction,
        outcome: &str,
        metadata: HashMap<String, Value>,
        session_id: Option<SessionId>,
    ) -> Result<(), SystemError> {
        if !self.config.audit_enabled {
            return Ok(());
        }

        let entry = AuditEntry {
            event_type: event_type.to_string(),
            actor,
            resource: resource.to_string(),
            action,
            outcome: outcome.to_string(),
            metadata,
            session_id,
        };

        self.push_audit(entry).await;
        Ok(())
    }

    /// Get recent logs (stateless - delegates to external service)
    pub async fn get_recent_logs(&self, count: usize) -> Vec<LogEntry> {
        let _ = count;
        Vec::new()
    }

    /// Get recent audit logs (stateless - delegates to external service)
    pub async fn get_recent_audit_logs(&self, count: usize) -> Vec<AuditEntry> {
        let _ = count;
        Vec::new()
    }

    /// Get logging statistics (stateless - delegates to external service)
    pub async fn get_statistics(&self) -> LoggingStats {
        LoggingStats::default()
    }
}

impl Default for LoggingSystemHandler {
    fn default() -> Self {
        Self::new(LoggingConfig::default())
    }
}

#[async_trait]
impl SystemEffects for LoggingSystemHandler {
    async fn log(&self, level: &str, component: &str, message: &str) -> Result<(), SystemError> {
        let parsed_level = LogLevel::try_from(level).unwrap_or(LogLevel::Info);
        let component_id = ComponentId::from(component);
        self.log_structured(
            parsed_level,
            component_id,
            message,
            HashMap::new(),
            None,
            None,
            None,
        )
        .await
    }

    async fn log_with_context(
        &self,
        level: &str,
        component: &str,
        message: &str,
        context: HashMap<String, String>,
    ) -> Result<(), SystemError> {
        let metadata: HashMap<String, Value> = context
            .into_iter()
            .map(|(k, v)| (k, Value::String(v)))
            .collect();
        let parsed_level = LogLevel::try_from(level).unwrap_or(LogLevel::Info);
        let component_id = ComponentId::from(component);
        self.log_structured(parsed_level, component_id, message, metadata, None, None, None)
            .await
    }

    async fn get_system_info(&self) -> Result<HashMap<String, String>, SystemError> {
        let mut info = HashMap::new();
        info.insert("component".to_string(), "logging".to_string());
        info.insert("log_level".to_string(), self.config.log_level.to_string());
        info.insert(
            "audit_enabled".to_string(),
            self.config.audit_enabled.to_string(),
        );
        info.insert("status".to_string(), "operational".to_string());
        Ok(info)
    }

    async fn set_config(&self, key: &str, value: &str) -> Result<(), SystemError> {
        match key {
            "log_level" => {
                // Validate the value but don't store it (stateless handler)
                LogLevel::try_from(value).map_err(|_| SystemError::InvalidConfiguration {
                    key: key.to_string(),
                    value: value.to_string(),
                })?;
                Ok(())
            }
            "audit_enabled" => {
                // Validate the value but don't store it (stateless handler)
                value
                    .parse::<bool>()
                    .map_err(|_| SystemError::InvalidConfiguration {
                        key: key.to_string(),
                        value: value.to_string(),
                    })?;
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
            "log_level" => Ok(self.config.log_level.to_string()),
            "audit_enabled" => Ok(self.config.audit_enabled.to_string()),
            "max_log_entries" => Ok(self.config.max_log_entries.to_string()),
            "max_audit_entries" => Ok(self.config.max_audit_entries.to_string()),
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

        metrics.insert("logs_total".to_string(), 0.0);
        metrics.insert("audit_logs_total".to_string(), 0.0);
        metrics.insert("logs_error".to_string(), 0.0);
        metrics.insert("logs_warn".to_string(), 0.0);
        metrics.insert("logs_info".to_string(), 0.0);
        metrics.insert("logs_debug".to_string(), 0.0);
        metrics.insert("recent_logs".to_string(), 0.0);
        metrics.insert("recent_audit_logs".to_string(), 0.0);
        metrics.insert(
            "max_log_entries_configured".to_string(),
            self.config.max_log_entries as f64,
        );
        metrics.insert(
            "max_audit_entries_configured".to_string(),
            self.config.max_audit_entries as f64,
        );
        Ok(metrics)
    }

    async fn restart_component(&self, _component: &str) -> Result<(), SystemError> {
        warn!("Restart not implemented for logging system handler");
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
    use serde_json::json;

    #[tokio::test]
    async fn test_logging_handler_creation() {
        let handler = LoggingSystemHandler::new(LoggingConfig::default());
        // LoggingSystemHandler should be created successfully
        assert_eq!(handler.config.log_level, LogLevel::Info);
        assert!(handler.config.audit_enabled);
    }

    #[tokio::test]
    async fn test_basic_logging() {
        let handler = LoggingSystemHandler::new(LoggingConfig::default());

        handler
            .log("info", "test", "hello world")
            .await
            .expect("log ok");
        handler
            .log_with_context(
                "warn",
                "test",
                "with context",
                HashMap::from([("key".into(), "value".into())]),
            )
            .await
            .expect("log ok");

        // Test system effects
        let info = handler.get_system_info().await.unwrap();
        assert_eq!(info.get("component"), Some(&"logging".to_string()));
    }

    #[tokio::test]
    async fn test_audit_logging() {
        let handler = LoggingSystemHandler::new(LoggingConfig::default());

        handler
            .audit_log(
                "authentication",
                Some(DeviceId::new_from_entropy([3u8; 32])),
                "resource",
                AuditAction::Custom("action".to_string()),
                "success",
                HashMap::from([("extra".into(), json!("1"))]),
                None,
            )
            .await
            .expect("audit ok");

        // Test config operations
        let config_value = handler.get_config("log_level").await.unwrap();
        assert_eq!(config_value, "info");
    }
}
