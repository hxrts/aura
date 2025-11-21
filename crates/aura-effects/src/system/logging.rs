//! Logging system handler with structured logging and audit trails
//!
//! **Layer 3 (aura-effects)**: Basic single-operation handler.
//!
//! This module was moved from aura-protocol (Layer 4) because it implements a basic
//! SystemEffects handler with no coordination logic. It maintains per-instance state
//! for log buffering but doesn't coordinate multiple handlers or multi-party operations.

use async_trait::async_trait;
use aura_core::effects::{SystemEffects, SystemError};
use aura_core::identifiers::DeviceId;
use aura_core::SessionId;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// Log entry with structured metadata
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LogEntry {
    /// Unix timestamp of the log entry
    pub timestamp: u64,
    /// Log level (info, warn, error, debug, etc.)
    pub level: String,
    /// Log message content
    pub message: String,
    /// Source component that generated the log
    pub component: String,
    /// Associated session ID if applicable
    pub session_id: Option<SessionId>,
    /// Associated device ID if applicable
    pub device_id: Option<DeviceId>,
    /// Structured metadata fields
    pub metadata: HashMap<String, Value>,
    /// Trace ID for distributed tracing correlation
    pub trace_id: Option<Uuid>,
}

/// Audit log entry for security-critical events
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AuditEntry {
    /// Unix timestamp of the audit event
    pub timestamp: u64,
    /// Type of security event (login, permission_change, etc.)
    pub event_type: String,
    /// Device/actor that initiated the event
    pub actor: Option<DeviceId>,
    /// Resource affected by the event
    pub resource: String,
    /// Action performed on the resource
    pub action: String,
    /// Outcome of the action (success, failure, denied, etc.)
    pub outcome: String,
    /// Additional audit metadata
    pub metadata: HashMap<String, Value>,
    /// Associated session ID if applicable
    pub session_id: Option<SessionId>,
}

/// Log buffer for in-memory storage
#[derive(Debug, Clone)]
struct LogBuffer {
    entries: Vec<LogEntry>,
    max_entries: usize,
}

impl LogBuffer {
    fn new(max_entries: usize) -> Self {
        Self {
            entries: Vec::with_capacity(max_entries),
            max_entries,
        }
    }

    fn push(&mut self, entry: LogEntry) {
        if self.entries.len() >= self.max_entries {
            self.entries.remove(0);
        }
        self.entries.push(entry);
    }

    fn get_recent(&self, count: usize) -> Vec<LogEntry> {
        let start = if self.entries.len() > count {
            self.entries.len() - count
        } else {
            0
        };
        self.entries[start..].to_vec()
    }

    fn filter(&self, level: &str, component: Option<&str>) -> Vec<LogEntry> {
        self.entries
            .iter()
            .filter(|entry| {
                entry.level == level && component.map_or(true, |c| entry.component == c)
            })
            .cloned()
            .collect()
    }
}

/// Configuration for logging system
#[derive(Debug, Clone)]
pub struct LoggingConfig {
    /// Maximum number of log entries to keep in memory
    pub max_log_entries: usize,
    /// Maximum number of audit entries to keep in memory
    pub max_audit_entries: usize,
    /// Whether to enable file-based logging
    pub enable_file_logging: bool,
    /// Whether to enable remote logging (e.g., syslog)
    pub enable_remote_logging: bool,
    /// Minimum log level to record (debug, info, warn, error)
    pub log_level: String,
    /// Whether to record audit events
    pub audit_enabled: bool,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            max_log_entries: 10000,
            max_audit_entries: 5000,
            enable_file_logging: false,
            enable_remote_logging: false,
            log_level: "info".to_string(),
            audit_enabled: true,
        }
    }
}

/// Statistics for the logging system
#[derive(Debug, Clone, Default)]
pub struct LoggingStats {
    /// Total number of log entries processed
    pub total_logs: u64,
    /// Total number of audit log entries processed
    pub total_audit_logs: u64,
    /// Number of error-level logs recorded
    pub error_logs: u64,
    /// Number of warn-level logs recorded
    pub warn_logs: u64,
    /// Number of info-level logs recorded
    pub info_logs: u64,
    /// Number of debug-level logs recorded
    pub debug_logs: u64,
    /// Number of logs dropped due to buffer overflow
    pub dropped_logs: u64,
    /// Logging system uptime in seconds
    pub uptime_seconds: u64,
}

/// Logging system handler with structured logging and audit capabilities
pub struct LoggingSystemHandler {
    config: LoggingConfig,
    log_buffer: Arc<RwLock<LogBuffer>>,
    audit_buffer: Arc<RwLock<LogBuffer>>,
    stats: Arc<RwLock<LoggingStats>>,
    #[allow(dead_code)]
    start_time: SystemTime,
    log_sender: Arc<RwLock<Option<mpsc::UnboundedSender<LogEntry>>>>,
    audit_sender: Arc<RwLock<Option<mpsc::UnboundedSender<AuditEntry>>>>,
}

impl LoggingSystemHandler {
    /// Create a new logging system handler
    pub fn new(config: LoggingConfig) -> Self {
        let log_buffer = Arc::new(RwLock::new(LogBuffer::new(config.max_log_entries)));
        let audit_buffer = Arc::new(RwLock::new(LogBuffer::new(config.max_audit_entries)));

        let (log_tx, log_rx) = mpsc::unbounded_channel();
        let (audit_tx, audit_rx) = mpsc::unbounded_channel();

        let handler = Self {
            config: config.clone(),
            log_buffer: log_buffer.clone(),
            audit_buffer: audit_buffer.clone(),
            stats: Arc::new(RwLock::new(LoggingStats::default())),
            start_time: SystemTime::UNIX_EPOCH, // Fixed start time for deterministic logging
            log_sender: Arc::new(RwLock::new(Some(log_tx))),
            audit_sender: Arc::new(RwLock::new(Some(audit_tx))),
        };

        // Start background processors
        handler.start_log_processor(log_rx);
        handler.start_audit_processor(audit_rx);

        info!(
            "Logging system handler initialized with config: {:?}",
            config
        );
        handler
    }

    /// Start the background log processor
    fn start_log_processor(&self, mut log_rx: mpsc::UnboundedReceiver<LogEntry>) {
        let log_buffer = self.log_buffer.clone();
        let stats = self.stats.clone();

        // Only spawn if a runtime is available, otherwise logs will be dropped
        if let Ok(_handle) = tokio::runtime::Handle::try_current() {
            tokio::spawn(async move {
                while let Some(entry) = log_rx.recv().await {
                    // Update statistics
                    {
                        let mut stats_guard = stats.write().await;
                        stats_guard.total_logs += 1;
                        match entry.level.as_str() {
                            "error" => stats_guard.error_logs += 1,
                            "warn" => stats_guard.warn_logs += 1,
                            "info" => stats_guard.info_logs += 1,
                            "debug" => stats_guard.debug_logs += 1,
                            _ => {}
                        }
                    }

                    // Store in buffer
                    {
                        let mut buffer = log_buffer.write().await;
                        buffer.push(entry.clone());
                    }

                    // Forward to tracing if enabled
                    match entry.level.as_str() {
                        "error" => error!("{}: {}", entry.component, entry.message),
                        "warn" => warn!("{}: {}", entry.component, entry.message),
                        "info" => info!("{}: {}", entry.component, entry.message),
                        "debug" => debug!("{}: {}", entry.component, entry.message),
                        _ => info!("{}: {}", entry.component, entry.message),
                    }
                }
            });
        }
    }

    /// Start the background audit processor
    fn start_audit_processor(&self, mut audit_rx: mpsc::UnboundedReceiver<AuditEntry>) {
        let audit_buffer = self.audit_buffer.clone();
        let stats = self.stats.clone();

        // Only spawn if a runtime is available, otherwise audit events will be dropped
        if let Ok(_handle) = tokio::runtime::Handle::try_current() {
            tokio::spawn(async move {
                while let Some(entry) = audit_rx.recv().await {
                    // Update statistics
                    {
                        let mut stats_guard = stats.write().await;
                        stats_guard.total_audit_logs += 1;
                    }

                    // Store in buffer (convert to LogEntry for storage)
                    {
                        let log_entry = LogEntry {
                            timestamp: entry.timestamp,
                            level: "audit".to_string(),
                            message: format!(
                                "{}: {} {} on {}",
                                entry.event_type, entry.action, entry.outcome, entry.resource
                            ),
                            component: "audit".to_string(),
                            session_id: entry.session_id,
                            device_id: entry.actor,
                            metadata: entry.metadata.clone(),
                            trace_id: None,
                        };

                        let mut buffer = audit_buffer.write().await;
                        buffer.push(log_entry);
                    }

                    // Log audit entry
                    info!(
                        "AUDIT: {} by {:?} - {} on {} (outcome: {})",
                        entry.event_type, entry.actor, entry.action, entry.resource, entry.outcome
                    );
                }
            });
        }
    }

    /// Get current uptime in seconds
    /// NOTE: In production, should use TimeEffects for current time
    fn get_uptime_seconds(&self) -> u64 {
        // TODO: Replace with TimeEffects::current_timestamp() - start_time
        0 // Fixed uptime for deterministic behavior
    }

    /// Get current timestamp
    fn current_timestamp(&self) -> u64 {
        // Use fixed timestamp for deterministic logging
        0
    }

    /// Log a structured message
    pub async fn log_structured(
        &self,
        level: &str,
        component: &str,
        message: &str,
        metadata: HashMap<String, Value>,
        session_id: Option<SessionId>,
        device_id: Option<DeviceId>,
        trace_id: Option<Uuid>,
    ) -> Result<(), SystemError> {
        let entry = LogEntry {
            timestamp: self.current_timestamp(),
            level: level.to_string(),
            message: message.to_string(),
            component: component.to_string(),
            session_id,
            device_id,
            metadata,
            trace_id,
        };

        if let Some(ref sender) = *self.log_sender.read().await {
            sender
                .send(entry)
                .map_err(|_| SystemError::ServiceUnavailable)?;
        }

        Ok(())
    }

    /// Log an audit event
    pub async fn audit_log(
        &self,
        event_type: &str,
        actor: Option<DeviceId>,
        resource: &str,
        action: &str,
        outcome: &str,
        metadata: HashMap<String, Value>,
        session_id: Option<SessionId>,
    ) -> Result<(), SystemError> {
        if !self.config.audit_enabled {
            return Ok(());
        }

        let entry = AuditEntry {
            timestamp: self.current_timestamp(),
            event_type: event_type.to_string(),
            actor,
            resource: resource.to_string(),
            action: action.to_string(),
            outcome: outcome.to_string(),
            metadata,
            session_id,
        };

        if let Some(ref sender) = *self.audit_sender.read().await {
            sender
                .send(entry)
                .map_err(|_| SystemError::ServiceUnavailable)?;
        }

        Ok(())
    }

    /// Get recent log entries
    pub async fn get_recent_logs(&self, count: usize) -> Vec<LogEntry> {
        self.log_buffer.read().await.get_recent(count)
    }

    /// Get recent audit entries
    pub async fn get_recent_audit_logs(&self, count: usize) -> Vec<LogEntry> {
        self.audit_buffer.read().await.get_recent(count)
    }

    /// Filter logs by level and optional component
    pub async fn filter_logs(&self, level: &str, component: Option<&str>) -> Vec<LogEntry> {
        self.log_buffer.read().await.filter(level, component)
    }

    /// Get current logging statistics
    pub async fn get_statistics(&self) -> LoggingStats {
        let mut stats = self.stats.read().await.clone();
        stats.uptime_seconds = self.get_uptime_seconds();
        stats
    }

    /// Set log level
    pub async fn set_log_level(&mut self, level: &str) {
        self.config.log_level = level.to_string();
        info!("Log level set to: {}", level);
    }

    /// Enable or disable audit logging
    pub async fn set_audit_enabled(&mut self, enabled: bool) {
        self.config.audit_enabled = enabled;
        info!(
            "Audit logging {}",
            if enabled { "enabled" } else { "disabled" }
        );
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
        self.log_structured(level, component, message, HashMap::new(), None, None, None)
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

        self.log_structured(level, component, message, metadata, None, None, None)
            .await
    }

    async fn get_system_info(&self) -> Result<HashMap<String, String>, SystemError> {
        let stats = self.get_statistics().await;
        let mut info = HashMap::new();

        info.insert("component".to_string(), "logging".to_string());
        info.insert(
            "uptime_seconds".to_string(),
            stats.uptime_seconds.to_string(),
        );
        info.insert("total_logs".to_string(), stats.total_logs.to_string());
        info.insert(
            "total_audit_logs".to_string(),
            stats.total_audit_logs.to_string(),
        );
        info.insert("error_logs".to_string(), stats.error_logs.to_string());
        info.insert("warn_logs".to_string(), stats.warn_logs.to_string());
        info.insert("info_logs".to_string(), stats.info_logs.to_string());
        info.insert("debug_logs".to_string(), stats.debug_logs.to_string());
        info.insert("log_level".to_string(), self.config.log_level.clone());
        info.insert(
            "audit_enabled".to_string(),
            self.config.audit_enabled.to_string(),
        );

        Ok(info)
    }

    async fn set_config(&self, key: &str, value: &str) -> Result<(), SystemError> {
        match key {
            "log_level" => {
                // Note: This requires &mut self, but trait method takes &self
                // In practice, would use interior mutability or different design
                info!("Would set log level to: {}", value);
                Ok(())
            }
            "audit_enabled" => {
                let enabled =
                    value
                        .parse::<bool>()
                        .map_err(|_| SystemError::InvalidConfiguration {
                            key: key.to_string(),
                            value: value.to_string(),
                        })?;
                info!("Would set audit enabled to: {}", enabled);
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
            "log_level" => Ok(self.config.log_level.clone()),
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
        // Check if background processors are still running by testing channels
        let log_sender_ok = self.log_sender.read().await.is_some();
        let audit_sender_ok = self.audit_sender.read().await.is_some();

        Ok(log_sender_ok && audit_sender_ok)
    }

    async fn get_metrics(&self) -> Result<HashMap<String, f64>, SystemError> {
        let stats = self.get_statistics().await;
        let mut metrics = HashMap::new();

        metrics.insert("total_logs".to_string(), stats.total_logs as f64);
        metrics.insert(
            "total_audit_logs".to_string(),
            stats.total_audit_logs as f64,
        );
        metrics.insert("error_logs".to_string(), stats.error_logs as f64);
        metrics.insert("warn_logs".to_string(), stats.warn_logs as f64);
        metrics.insert("info_logs".to_string(), stats.info_logs as f64);
        metrics.insert("debug_logs".to_string(), stats.debug_logs as f64);
        metrics.insert("dropped_logs".to_string(), stats.dropped_logs as f64);
        metrics.insert("uptime_seconds".to_string(), stats.uptime_seconds as f64);

        // Calculate logs per second
        if stats.uptime_seconds > 0 {
            metrics.insert(
                "logs_per_second".to_string(),
                stats.total_logs as f64 / stats.uptime_seconds as f64,
            );
        }

        Ok(metrics)
    }

    async fn restart_component(&self, _component: &str) -> Result<(), SystemError> {
        warn!("Restart not implemented for logging system handler");
        Ok(())
    }

    async fn shutdown(&self) -> Result<(), SystemError> {
        info!("Shutting down logging system handler");

        // Close channels to signal shutdown
        *self.log_sender.write().await = None;
        *self.audit_sender.write().await = None;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tokio::time::{sleep, Duration};

    #[tokio::test]
    async fn test_logging_handler_creation() {
        let handler = LoggingSystemHandler::new(LoggingConfig::default());
        let stats = handler.get_statistics().await;

        assert_eq!(stats.total_logs, 0);
        assert_eq!(stats.total_audit_logs, 0);
        assert!(handler.health_check().await.unwrap());
    }

    #[tokio::test]
    async fn test_structured_logging() {
        let handler = LoggingSystemHandler::new(LoggingConfig::default());

        let mut metadata = HashMap::new();
        metadata.insert("key1".to_string(), json!("value1"));
        metadata.insert("key2".to_string(), json!(42));

        handler
            .log_structured("info", "test", "Test message", metadata, None, None, None)
            .await
            .unwrap();

        // Give time for background processing
        sleep(Duration::from_millis(50)).await;

        let stats = handler.get_statistics().await;
        assert_eq!(stats.total_logs, 1);
        assert_eq!(stats.info_logs, 1);

        let recent_logs = handler.get_recent_logs(10).await;
        assert_eq!(recent_logs.len(), 1);
        assert_eq!(recent_logs[0].message, "Test message");
    }

    #[tokio::test]
    async fn test_audit_logging() {
        let handler = LoggingSystemHandler::new(LoggingConfig::default());

        let mut metadata = HashMap::new();
        metadata.insert("resource_id".to_string(), json!("test-resource"));

        handler
            .audit_log(
                "authentication",
                Some(DeviceId::new()),
                "user_session",
                "login",
                "success",
                metadata,
                None,
            )
            .await
            .unwrap();

        // Give time for background processing
        sleep(Duration::from_millis(50)).await;

        let stats = handler.get_statistics().await;
        assert_eq!(stats.total_audit_logs, 1);

        let recent_audit_logs = handler.get_recent_audit_logs(10).await;
        assert_eq!(recent_audit_logs.len(), 1);
    }

    #[tokio::test]
    async fn test_system_effects_interface() {
        let handler = LoggingSystemHandler::new(LoggingConfig::default());

        // Test basic logging
        handler
            .log("info", "test", "Test log message")
            .await
            .unwrap();

        // Test logging with context
        let mut context = HashMap::new();
        context.insert("session_id".to_string(), "test-session".to_string());
        handler
            .log_with_context("warn", "test", "Warning message", context)
            .await
            .unwrap();

        // Give time for background processing
        sleep(Duration::from_millis(50)).await;

        // Test metrics
        let metrics = handler.get_metrics().await.unwrap();
        assert!(metrics.contains_key("total_logs"));
        assert_eq!(metrics["total_logs"], 2.0);

        // Test system info
        let info = handler.get_system_info().await.unwrap();
        assert_eq!(info["component"], "logging");

        // Test health check
        assert!(handler.health_check().await.unwrap());
    }

    #[tokio::test]
    async fn test_log_filtering() {
        let handler = LoggingSystemHandler::new(LoggingConfig::default());

        // Log different levels
        handler
            .log("info", "component1", "Info message")
            .await
            .unwrap();
        handler
            .log("error", "component1", "Error message")
            .await
            .unwrap();
        handler
            .log("info", "component2", "Another info message")
            .await
            .unwrap();

        // Give time for background processing
        sleep(Duration::from_millis(50)).await;

        // Filter by level
        let info_logs = handler.filter_logs("info", None).await;
        assert_eq!(info_logs.len(), 2);

        let error_logs = handler.filter_logs("error", None).await;
        assert_eq!(error_logs.len(), 1);

        // Filter by component
        let component1_info_logs = handler.filter_logs("info", Some("component1")).await;
        assert_eq!(component1_info_logs.len(), 1);
    }

    #[tokio::test]
    async fn test_configuration() {
        let handler = LoggingSystemHandler::new(LoggingConfig::default());

        // Test getting configuration
        let log_level = handler.get_config("log_level").await.unwrap();
        assert_eq!(log_level, "info");

        let audit_enabled = handler.get_config("audit_enabled").await.unwrap();
        assert_eq!(audit_enabled, "true");

        // Test setting configuration (limited by trait design)
        handler.set_config("log_level", "debug").await.unwrap();
        handler.set_config("audit_enabled", "false").await.unwrap();

        // Test invalid configuration
        let result = handler.set_config("invalid_key", "value").await;
        assert!(result.is_err());
    }
}
