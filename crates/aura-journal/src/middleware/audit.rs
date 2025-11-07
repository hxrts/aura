//! Audit logging middleware for compliance tracking

use super::{JournalContext, JournalHandler, JournalMiddleware};
use crate::error::{Error, Result};
use crate::operations::JournalOperation;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

/// Audit middleware that logs all journal operations for compliance
pub struct AuditMiddleware {
    /// Audit logger
    logger: Box<dyn AuditLogger>,

    /// Configuration
    config: AuditConfig,
}

impl AuditMiddleware {
    /// Create new audit middleware
    pub fn new(logger: Box<dyn AuditLogger>, config: AuditConfig) -> Self {
        Self { logger, config }
    }

    /// Create middleware with console audit logger
    pub fn with_console_logger(config: AuditConfig) -> Self {
        Self::new(Box::new(ConsoleAuditLogger::new()), config)
    }
}

impl JournalMiddleware for AuditMiddleware {
    // [VERIFIED] Uses SystemTime for audit timing measurements
    #[allow(clippy::disallowed_methods)]
    fn process(
        &self,
        operation: JournalOperation,
        context: &JournalContext,
        next: &dyn JournalHandler,
    ) -> Result<serde_json::Value> {
        // Skip audit logging if disabled
        if !self.config.enable_audit_logging {
            return next.handle(operation, context);
        }

        let start_time = SystemTime::now();

        // Create audit entry for operation start
        let mut audit_entry = AuditEntry::new(
            context.account_id.to_string(),
            context.device_id.to_string(),
            format!("{:?}", operation),
            AuditEventType::OperationStarted,
        );

        // Add operation-specific details
        audit_entry.add_detail("operation_type", &context.operation_type);
        audit_entry.add_detail("timestamp", &context.timestamp.to_string());

        // Add metadata
        for (key, value) in &context.metadata {
            audit_entry.add_detail(key, value);
        }

        // Log operation start
        if self.config.log_operation_start {
            self.logger.log_audit_entry(&audit_entry)?;
        }

        // Execute operation
        let result = next.handle(operation.clone(), context);

        let duration = start_time.elapsed().unwrap_or_default();

        // Create audit entry for operation completion
        let mut completion_entry = AuditEntry::new(
            context.account_id.to_string(),
            context.device_id.to_string(),
            format!("{:?}", operation),
            match &result {
                Ok(_) => AuditEventType::OperationSucceeded,
                Err(_) => AuditEventType::OperationFailed,
            },
        );

        completion_entry.add_detail("duration_ms", &duration.as_millis().to_string());

        // Add result details
        match &result {
            Ok(response) => {
                if self.config.log_operation_results {
                    completion_entry.add_detail("result", &response.to_string());
                }
            }
            Err(error) => {
                completion_entry.add_detail("error", &error.to_string());
                completion_entry.add_detail("error_type", "journal_operation_error");
            }
        }

        // Log operation completion
        self.logger.log_audit_entry(&completion_entry)?;

        result
    }

    fn name(&self) -> &str {
        "audit"
    }
}

/// Configuration for audit middleware
#[derive(Debug, Clone)]
pub struct AuditConfig {
    /// Whether audit logging is enabled
    pub enable_audit_logging: bool,

    /// Whether to log operation start events
    pub log_operation_start: bool,

    /// Whether to log operation results (may contain sensitive data)
    pub log_operation_results: bool,

    /// Whether to log failed operations with detailed error info
    pub log_detailed_errors: bool,

    /// Maximum detail value length before truncation
    pub max_detail_length: usize,
}

impl Default for AuditConfig {
    fn default() -> Self {
        Self {
            enable_audit_logging: true,
            log_operation_start: false,   // Reduce log volume
            log_operation_results: false, // Avoid logging sensitive data
            log_detailed_errors: true,
            max_detail_length: 1000,
        }
    }
}

/// Audit entry representing a logged event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    /// Unique audit entry ID
    pub id: String,

    /// Account ID
    pub account_id: String,

    /// Device ID that performed the operation
    pub device_id: String,

    /// Operation or event description
    pub operation: String,

    /// Type of audit event
    pub event_type: AuditEventType,

    /// Timestamp when the event occurred
    pub timestamp: u64,

    /// Additional details about the event
    pub details: std::collections::HashMap<String, String>,
}

impl AuditEntry {
    /// Create a new audit entry
    ///
    /// [VERIFIED] Uses SystemTime and UUID for audit trail creation
    #[allow(clippy::disallowed_methods)]
    pub fn new(
        account_id: String,
        device_id: String,
        operation: String,
        event_type: AuditEventType,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            account_id,
            device_id,
            operation,
            event_type,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            details: std::collections::HashMap::new(),
        }
    }

    /// Add a detail to the audit entry
    pub fn add_detail(&mut self, key: &str, value: &str) {
        self.details.insert(key.to_string(), value.to_string());
    }
}

/// Types of audit events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AuditEventType {
    /// Operation was started
    OperationStarted,

    /// Operation completed successfully
    OperationSucceeded,

    /// Operation failed
    OperationFailed,

    /// Security event (unauthorized access attempt, etc.)
    SecurityEvent,

    /// Configuration change
    ConfigurationChange,

    /// Custom event type
    Custom(String),
}

/// Trait for audit logging backends
pub trait AuditLogger: Send + Sync {
    /// Log an audit entry
    fn log_audit_entry(&self, entry: &AuditEntry) -> Result<()>;

    /// Query audit entries (optional, for compliance reporting)
    fn query_audit_entries(
        &self,
        _account_id: &str,
        _start_time: Option<u64>,
        _end_time: Option<u64>,
    ) -> Result<Vec<AuditEntry>> {
        // Default implementation returns empty - not all loggers support querying
        Ok(Vec::new())
    }
}

/// Console audit logger for development and testing
pub struct ConsoleAuditLogger {
    /// Whether to format output as JSON
    json_format: bool,
}

impl ConsoleAuditLogger {
    /// Create a new console audit logger
    pub fn new() -> Self {
        Self { json_format: false }
    }

    /// Create a console logger with JSON formatting
    pub fn with_json_format() -> Self {
        Self { json_format: true }
    }
}

impl Default for ConsoleAuditLogger {
    fn default() -> Self {
        Self::new()
    }
}

impl AuditLogger for ConsoleAuditLogger {
    fn log_audit_entry(&self, entry: &AuditEntry) -> Result<()> {
        if self.json_format {
            println!(
                "{}",
                serde_json::to_string(entry).map_err(|e| {
                    Error::storage_failed(format!("Failed to serialize audit entry: {}", e))
                })?
            );
        } else {
            println!(
                "[AUDIT] {} | {} | {} | {} | {:?} | {} details",
                entry.timestamp,
                entry.account_id,
                entry.device_id,
                entry.operation,
                entry.event_type,
                entry.details.len()
            );
        }
        Ok(())
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
    fn test_audit_middleware() {
        let effects = Effects::test(42);
        let account_id = aura_types::AccountId::new_with_effects(&effects);
        let device_id = aura_types::DeviceId::new_with_effects(&effects);

        let middleware = AuditMiddleware::with_console_logger(AuditConfig::default());
        let handler = NoOpHandler;
        let context = JournalContext::new(account_id, device_id, "test".to_string());
        let operation = JournalOperation::GetEpoch;

        // Process operation - should succeed and log audit entries
        let result = middleware.process(operation, &context, &handler);
        assert!(result.is_ok());
    }

    #[test]
    fn test_audit_entry_creation() {
        let entry = AuditEntry::new(
            "account123".to_string(),
            "device456".to_string(),
            "GetEpoch".to_string(),
            AuditEventType::OperationSucceeded,
        );

        assert_eq!(entry.account_id, "account123");
        assert_eq!(entry.device_id, "device456");
        assert_eq!(entry.operation, "GetEpoch");
        assert!(entry.id.len() > 0);
        assert!(entry.timestamp > 0);
    }
}
