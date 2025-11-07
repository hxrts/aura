//! Audit logging middleware for cryptographic operation tracking

use super::{CryptoContext, CryptoHandler, CryptoMiddleware, SecurityLevel};
use crate::middleware::CryptoOperation;
use crate::{CryptoError, Result};
use aura_types::DeviceId;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Audit logging middleware that tracks cryptographic operations for compliance
pub struct AuditLoggingMiddleware {
    /// Audit log storage
    log_storage: Arc<RwLock<AuditLogStorage>>,

    /// Configuration
    config: AuditConfig,
}

impl AuditLoggingMiddleware {
    /// Create new audit logging middleware
    pub fn new(config: AuditConfig) -> Self {
        Self {
            log_storage: Arc::new(RwLock::new(AuditLogStorage::new())),
            config,
        }
    }

    /// Get audit logging statistics
    pub fn stats(&self) -> AuditStats {
        let storage = self.log_storage.read().unwrap();
        storage.stats()
    }

    /// Query audit logs by criteria
    pub fn query_logs(&self, criteria: AuditQueryCriteria) -> Result<Vec<AuditLogEntry>> {
        let storage = self.log_storage.read().map_err(|_| {
            CryptoError::internal_error("Failed to acquire read lock on audit log storage")
        })?;

        Ok(storage.query_logs(criteria))
    }

    /// Export audit logs for compliance reporting
    pub fn export_logs(
        &self,
        format: AuditExportFormat,
        date_range: Option<(u64, u64)>,
    ) -> Result<String> {
        let storage = self.log_storage.read().map_err(|_| {
            CryptoError::internal_error("Failed to acquire read lock on audit log storage")
        })?;

        storage.export_logs(format, date_range)
    }

    /// Clean up old audit logs based on retention policy
    pub fn cleanup_old_logs(&self) -> Result<usize> {
        let mut storage = self.log_storage.write().map_err(|_| {
            CryptoError::internal_error("Failed to acquire write lock on audit log storage")
        })?;

        Ok(storage.cleanup_old_logs(self.config.retention_period))
    }
}

impl CryptoMiddleware for AuditLoggingMiddleware {
    fn process(
        &self,
        operation: CryptoOperation,
        context: &CryptoContext,
        next: &dyn CryptoHandler,
    ) -> Result<serde_json::Value> {
        // Check if this operation should be audited
        let should_audit = self.should_audit_operation(&operation, context);

        if !should_audit {
            // Pass through without auditing
            return next.handle(operation, context);
        }

        #[allow(clippy::disallowed_methods)] // [VERIFIED] Acceptable in audit logging timestamp
        let start_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        #[allow(clippy::disallowed_methods)]
        // [VERIFIED] Acceptable in audit operation ID generation
        let operation_id = uuid::Uuid::new_v4().to_string();

        // Create initial audit log entry
        #[allow(clippy::disallowed_methods)] // [VERIFIED] Acceptable in audit entry ID generation
        let entry_id = uuid::Uuid::new_v4().to_string();
        let mut audit_entry = AuditLogEntry {
            entry_id,
            operation_id: operation_id.clone(),
            operation_type: self.operation_type_string(&operation),
            account_id: context.account_id.to_string(),
            device_id: context.device_id.clone(),
            security_level: context.security_level.clone(),
            session_context: context.session_context.clone(),
            timestamp: start_time,
            duration_ms: 0,
            success: false,
            error_code: None,
            error_message: None,
            operation_details: self.extract_operation_details(&operation),
            result_summary: None,
            compliance_flags: self.get_compliance_flags(&operation, context),
        };

        // Log operation start if configured
        if self.config.log_operation_start {
            self.log_audit_entry(audit_entry.clone())?;
        }

        // Execute the operation
        let result = next.handle(operation.clone(), context);

        // Calculate operation duration
        #[allow(clippy::disallowed_methods)] // [VERIFIED] Acceptable in audit duration calculation
        let end_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        audit_entry.duration_ms = ((end_time - start_time) * 1000) as u32;

        // Update audit entry with result
        match &result {
            Ok(response) => {
                audit_entry.success = true;
                audit_entry.result_summary = Some(self.create_result_summary(response));

                // Add audit metadata to response if configured
                if self.config.include_audit_id_in_response {
                    if let Ok(mut enriched_response) =
                        serde_json::from_value::<serde_json::Value>(response.clone())
                    {
                        if let Some(obj) = enriched_response.as_object_mut() {
                            obj.insert(
                                "audit_entry_id".to_string(),
                                serde_json::Value::String(audit_entry.entry_id.clone()),
                            );
                            obj.insert(
                                "operation_id".to_string(),
                                serde_json::Value::String(operation_id),
                            );
                        }
                    }
                }
            }
            Err(error) => {
                audit_entry.success = false;
                audit_entry.error_code = Some(self.error_to_code(error));
                audit_entry.error_message = Some(error.to_string());

                // Check for security-relevant errors
                if self.is_security_relevant_error(error) {
                    audit_entry
                        .compliance_flags
                        .push("SECURITY_ERROR".to_string());
                }
            }
        }

        // Log final audit entry
        self.log_audit_entry(audit_entry)?;

        result
    }

    fn name(&self) -> &str {
        "audit_logging"
    }
}

impl AuditLoggingMiddleware {
    fn should_audit_operation(&self, operation: &CryptoOperation, context: &CryptoContext) -> bool {
        // Always audit critical security level operations
        if context.security_level >= SecurityLevel::Critical {
            return true;
        }

        // Audit based on operation type
        match operation {
            CryptoOperation::DeriveKey { .. } => self.config.audit_key_operations,
            CryptoOperation::GenerateSignature { .. } => self.config.audit_signature_operations,
            CryptoOperation::VerifySignature { .. } => self.config.audit_verification_operations,
            CryptoOperation::RotateKeys { .. } => true, // Always audit key rotation
            CryptoOperation::GenerateRandom { num_bytes } => {
                self.config.audit_random_operations
                    && *num_bytes >= self.config.random_audit_threshold
            }
            CryptoOperation::Encrypt { .. } | CryptoOperation::Decrypt { .. } => {
                self.config.audit_encryption_operations
            }
            CryptoOperation::Hash { .. } => false, // Hash operations are typically not audited
        }
    }

    fn operation_type_string(&self, operation: &CryptoOperation) -> String {
        match operation {
            CryptoOperation::DeriveKey { .. } => "DERIVE_KEY".to_string(),
            CryptoOperation::GenerateSignature { .. } => "GENERATE_SIGNATURE".to_string(),
            CryptoOperation::VerifySignature { .. } => "VERIFY_SIGNATURE".to_string(),
            CryptoOperation::GenerateRandom { .. } => "GENERATE_RANDOM".to_string(),
            CryptoOperation::RotateKeys { .. } => "ROTATE_KEYS".to_string(),
            CryptoOperation::Encrypt { .. } => "ENCRYPT".to_string(),
            CryptoOperation::Decrypt { .. } => "DECRYPT".to_string(),
            CryptoOperation::Hash { .. } => "HASH".to_string(),
        }
    }

    fn extract_operation_details(&self, operation: &CryptoOperation) -> serde_json::Value {
        match operation {
            CryptoOperation::DeriveKey {
                app_id,
                context,
                derivation_path,
            } => {
                serde_json::json!({
                    "app_id": app_id,
                    "context": context,
                    "derivation_path_length": derivation_path.len()
                })
            }

            CryptoOperation::GenerateSignature {
                message,
                signing_package,
            } => {
                serde_json::json!({
                    "message_hash": hex::encode(blake3::hash(message).as_bytes()),
                    "message_size": message.len(),
                    "signing_package_size": signing_package.len()
                })
            }

            CryptoOperation::VerifySignature {
                message,
                signature,
                public_key,
            } => {
                serde_json::json!({
                    "message_hash": hex::encode(blake3::hash(message).as_bytes()),
                    "message_size": message.len(),
                    "signature_size": signature.len(),
                    "public_key_size": public_key.len()
                })
            }

            CryptoOperation::GenerateRandom { num_bytes } => {
                serde_json::json!({
                    "num_bytes": num_bytes
                })
            }

            CryptoOperation::RotateKeys {
                old_threshold,
                new_threshold,
                new_participants,
            } => {
                serde_json::json!({
                    "old_threshold": old_threshold,
                    "new_threshold": new_threshold,
                    "participant_count": new_participants.len()
                })
            }

            CryptoOperation::Encrypt {
                plaintext,
                recipient_keys,
            } => {
                serde_json::json!({
                    "plaintext_size": plaintext.len(),
                    "recipient_count": recipient_keys.len()
                })
            }

            CryptoOperation::Decrypt {
                ciphertext,
                private_key,
            } => {
                serde_json::json!({
                    "ciphertext_size": ciphertext.len(),
                    "private_key_size": private_key.len()
                })
            }

            CryptoOperation::Hash { data, algorithm } => {
                serde_json::json!({
                    "data_size": data.len(),
                    "algorithm": algorithm
                })
            }
        }
    }

    fn get_compliance_flags(
        &self,
        operation: &CryptoOperation,
        context: &CryptoContext,
    ) -> Vec<String> {
        let mut flags = Vec::new();

        // Security level flags
        match context.security_level {
            SecurityLevel::Critical => flags.push("HIGH_SECURITY".to_string()),
            SecurityLevel::High => flags.push("ELEVATED_SECURITY".to_string()),
            _ => {}
        }

        // Operation type flags
        match operation {
            CryptoOperation::RotateKeys { .. } => {
                flags.push("KEY_LIFECYCLE".to_string());
                flags.push("CRITICAL_OPERATION".to_string());
            }
            CryptoOperation::GenerateSignature { .. } => {
                flags.push("SIGNATURE_GENERATION".to_string());
            }
            CryptoOperation::DeriveKey { .. } => {
                flags.push("KEY_DERIVATION".to_string());
            }
            _ => {}
        }

        // Add regulatory compliance flags if configured
        if self.config.include_regulatory_flags {
            flags.push("SOX_COMPLIANCE".to_string());
            flags.push("GDPR_RELEVANT".to_string());
        }

        flags
    }

    fn create_result_summary(&self, response: &serde_json::Value) -> String {
        // Create a summary of the operation result for audit purposes
        // Avoid logging sensitive data

        if let Some(operation) = response.get("operation").and_then(|v| v.as_str()) {
            if let Some(success) = response.get("success").and_then(|v| v.as_bool()) {
                format!(
                    "Operation {} {}",
                    operation,
                    if success { "succeeded" } else { "failed" }
                )
            } else {
                format!("Operation {} completed", operation)
            }
        } else {
            "Operation completed".to_string()
        }
    }

    fn error_to_code(&self, error: &CryptoError) -> String {
        // Use the standard error code system from AuraError
        if let Some(code) = error.code() {
            format!("{:?}", code)
        } else {
            "UNKNOWN".to_string()
        }
    }

    fn is_security_relevant_error(&self, error: &CryptoError) -> bool {
        // Use error severity to determine security relevance
        matches!(
            error.severity(),
            aura_types::ErrorSeverity::High | aura_types::ErrorSeverity::Critical
        )
    }

    fn log_audit_entry(&self, entry: AuditLogEntry) -> Result<()> {
        let mut storage = self.log_storage.write().map_err(|_| {
            CryptoError::internal_error("Failed to acquire write lock on audit log storage")
        })?;

        storage.add_entry(entry);

        // Trigger external logging if configured
        if self.config.enable_external_logging {
            // In real implementation, would send to external audit systems:
            // - SIEM systems
            // - Compliance logging services
            // - Cloud audit trails
            // - File-based audit logs
        }

        Ok(())
    }
}

/// Configuration for audit logging middleware
#[derive(Debug, Clone)]
pub struct AuditConfig {
    /// Whether to audit key operations
    pub audit_key_operations: bool,

    /// Whether to audit signature operations
    pub audit_signature_operations: bool,

    /// Whether to audit verification operations
    pub audit_verification_operations: bool,

    /// Whether to audit encryption operations
    pub audit_encryption_operations: bool,

    /// Whether to audit random generation operations
    pub audit_random_operations: bool,

    /// Minimum random bytes to trigger audit
    pub random_audit_threshold: usize,

    /// Whether to log operation start events
    pub log_operation_start: bool,

    /// Whether to include audit ID in operation responses
    pub include_audit_id_in_response: bool,

    /// Whether to include regulatory compliance flags
    pub include_regulatory_flags: bool,

    /// Whether to enable external logging
    pub enable_external_logging: bool,

    /// Audit log retention period
    pub retention_period: Duration,

    /// Maximum audit log entries to keep in memory
    pub max_memory_entries: usize,
}

impl Default for AuditConfig {
    fn default() -> Self {
        Self {
            audit_key_operations: true,
            audit_signature_operations: true,
            audit_verification_operations: false,
            audit_encryption_operations: true,
            audit_random_operations: false,
            random_audit_threshold: 1024, // 1KB
            log_operation_start: false,
            include_audit_id_in_response: false,
            include_regulatory_flags: false,
            enable_external_logging: false,
            retention_period: Duration::from_secs(90 * 24 * 60 * 60), // 90 days
            max_memory_entries: 10000,
        }
    }
}

/// Audit log entry
#[derive(Debug, Clone, serde::Serialize)]
pub struct AuditLogEntry {
    pub entry_id: String,
    pub operation_id: String,
    pub operation_type: String,
    pub account_id: String,
    pub device_id: DeviceId,
    pub security_level: SecurityLevel,
    pub session_context: String,
    pub timestamp: u64,
    pub duration_ms: u32,
    pub success: bool,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub operation_details: serde_json::Value,
    pub result_summary: Option<String>,
    pub compliance_flags: Vec<String>,
}

/// Audit query criteria
#[derive(Debug, Clone)]
pub struct AuditQueryCriteria {
    pub account_id: Option<String>,
    pub device_id: Option<DeviceId>,
    pub operation_type: Option<String>,
    pub security_level: Option<SecurityLevel>,
    pub success: Option<bool>,
    pub date_range: Option<(u64, u64)>,
    pub compliance_flags: Option<Vec<String>>,
    pub limit: Option<usize>,
}

/// Audit export format
#[derive(Debug, Clone)]
pub enum AuditExportFormat {
    Json,
    Csv,
    Xml,
}

/// Audit log storage
struct AuditLogStorage {
    entries: Vec<AuditLogEntry>,
    total_entries: u64,
    total_successful_operations: u64,
    total_failed_operations: u64,
    operation_counts: HashMap<String, u64>,
}

impl AuditLogStorage {
    fn new() -> Self {
        Self {
            entries: Vec::new(),
            total_entries: 0,
            total_successful_operations: 0,
            total_failed_operations: 0,
            operation_counts: HashMap::new(),
        }
    }

    fn add_entry(&mut self, entry: AuditLogEntry) {
        // Update statistics
        self.total_entries += 1;

        if entry.success {
            self.total_successful_operations += 1;
        } else {
            self.total_failed_operations += 1;
        }

        *self
            .operation_counts
            .entry(entry.operation_type.clone())
            .or_insert(0) += 1;

        // Add to storage
        self.entries.push(entry);

        // Implement simple LRU eviction if needed
        // In real implementation, would use persistent storage
    }

    fn query_logs(&self, criteria: AuditQueryCriteria) -> Vec<AuditLogEntry> {
        let mut results: Vec<AuditLogEntry> = self
            .entries
            .iter()
            .filter(|entry| {
                // Apply filters
                if let Some(ref account_id) = criteria.account_id {
                    if entry.account_id != *account_id {
                        return false;
                    }
                }

                if let Some(ref device_id) = criteria.device_id {
                    if entry.device_id != *device_id {
                        return false;
                    }
                }

                if let Some(ref operation_type) = criteria.operation_type {
                    if entry.operation_type != *operation_type {
                        return false;
                    }
                }

                if let Some(ref security_level) = criteria.security_level {
                    if entry.security_level != *security_level {
                        return false;
                    }
                }

                if let Some(success) = criteria.success {
                    if entry.success != success {
                        return false;
                    }
                }

                if let Some((start, end)) = criteria.date_range {
                    if entry.timestamp < start || entry.timestamp > end {
                        return false;
                    }
                }

                if let Some(ref flags) = criteria.compliance_flags {
                    if !flags
                        .iter()
                        .any(|flag| entry.compliance_flags.contains(flag))
                    {
                        return false;
                    }
                }

                true
            })
            .cloned()
            .collect();

        // Sort by timestamp (newest first)
        results.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

        // Apply limit
        if let Some(limit) = criteria.limit {
            results.truncate(limit);
        }

        results
    }

    fn export_logs(
        &self,
        format: AuditExportFormat,
        date_range: Option<(u64, u64)>,
    ) -> Result<String> {
        let mut entries_to_export = self.entries.clone();

        // Filter by date range if specified
        if let Some((start, end)) = date_range {
            entries_to_export.retain(|entry| entry.timestamp >= start && entry.timestamp <= end);
        }

        match format {
            AuditExportFormat::Json => serde_json::to_string_pretty(&entries_to_export)
                .map_err(|e| CryptoError::internal_error(format!("JSON export failed: {}", e))),

            AuditExportFormat::Csv => {
                let mut csv = String::new();
                csv.push_str("entry_id,operation_id,operation_type,account_id,device_id,security_level,timestamp,duration_ms,success,error_code,compliance_flags\n");

                for entry in entries_to_export {
                    csv.push_str(&format!(
                        "{},{},{},{},{},{:?},{},{},{},{},{}\n",
                        entry.entry_id,
                        entry.operation_id,
                        entry.operation_type,
                        entry.account_id,
                        entry.device_id,
                        entry.security_level,
                        entry.timestamp,
                        entry.duration_ms,
                        entry.success,
                        entry.error_code.unwrap_or_default(),
                        entry.compliance_flags.join(";")
                    ));
                }

                Ok(csv)
            }

            AuditExportFormat::Xml => {
                // Simplified XML export
                let mut xml = String::new();
                xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<audit_logs>\n");

                for entry in entries_to_export {
                    xml.push_str(&format!(
                        "  <entry id=\"{}\" operation_id=\"{}\" type=\"{}\" timestamp=\"{}\" success=\"{}\"/>\n",
                        entry.entry_id,
                        entry.operation_id,
                        entry.operation_type,
                        entry.timestamp,
                        entry.success
                    ));
                }

                xml.push_str("</audit_logs>\n");
                Ok(xml)
            }
        }
    }

    fn cleanup_old_logs(&mut self, retention_period: Duration) -> usize {
        #[allow(clippy::disallowed_methods)] // [VERIFIED] Acceptable in log retention cleanup
        let cutoff = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            - retention_period.as_secs();

        let original_count = self.entries.len();
        self.entries.retain(|entry| entry.timestamp >= cutoff);

        original_count - self.entries.len()
    }

    fn stats(&self) -> AuditStats {
        AuditStats {
            total_entries: self.total_entries,
            current_entries: self.entries.len(),
            total_successful_operations: self.total_successful_operations,
            total_failed_operations: self.total_failed_operations,
            operation_counts: self.operation_counts.clone(),
        }
    }
}

/// Audit logging statistics
#[derive(Debug, Clone)]
pub struct AuditStats {
    /// Total audit entries created
    pub total_entries: u64,

    /// Current entries in storage
    pub current_entries: usize,

    /// Total successful operations audited
    pub total_successful_operations: u64,

    /// Total failed operations audited
    pub total_failed_operations: u64,

    /// Operation counts by type
    pub operation_counts: HashMap<String, u64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::middleware::handler::NoOpHandler;
    use aura_crypto::Effects;
    use aura_types::{AccountIdExt, DeviceIdExt};

    #[test]
    fn test_audit_logging_middleware() {
        let effects = Effects::test(42);
        let account_id = aura_types::AccountId::new_with_effects(&effects);
        let device_id = aura_types::DeviceId::new_with_effects(&effects);

        let middleware = AuditLoggingMiddleware::new(AuditConfig::default());
        let handler = NoOpHandler;
        let context = CryptoContext::new(
            account_id,
            device_id,
            "test".to_string(),
            SecurityLevel::Critical,
        );
        let operation = CryptoOperation::DeriveKey {
            app_id: "test-app".to_string(),
            context: "test-context".to_string(),
            derivation_path: vec![0, 1, 2],
        };

        let result = middleware.process(operation, &context, &handler);
        assert!(result.is_ok());

        let stats = middleware.stats();
        assert_eq!(stats.total_entries, 1);
        assert_eq!(stats.total_successful_operations, 1);
    }

    #[test]
    fn test_audit_operation_filtering() {
        let config = AuditConfig {
            audit_key_operations: true,
            audit_signature_operations: false,
            ..AuditConfig::default()
        };
        let middleware = AuditLoggingMiddleware::new(config);

        let effects = Effects::test(42);
        let account_id = aura_types::AccountId::new_with_effects(&effects);
        let device_id = aura_types::DeviceId::new_with_effects(&effects);

        let context = CryptoContext::new(
            account_id,
            device_id,
            "test".to_string(),
            SecurityLevel::Standard,
        );

        // Key operation should be audited
        let key_op = CryptoOperation::DeriveKey {
            app_id: "test".to_string(),
            context: "test".to_string(),
            derivation_path: vec![],
        };
        assert!(middleware.should_audit_operation(&key_op, &context));

        // Signature operation should not be audited (disabled in config)
        let sig_op = CryptoOperation::GenerateSignature {
            message: b"test".to_vec(),
            signing_package: b"package".to_vec(),
        };
        assert!(!middleware.should_audit_operation(&sig_op, &context));
    }

    #[test]
    fn test_audit_query() {
        let middleware = AuditLoggingMiddleware::new(AuditConfig::default());

        // Query should return empty initially
        let criteria = AuditQueryCriteria {
            account_id: None,
            device_id: None,
            operation_type: Some("DERIVE_KEY".to_string()),
            security_level: None,
            success: None,
            date_range: None,
            compliance_flags: None,
            limit: None,
        };

        let results = middleware.query_logs(criteria).unwrap();
        assert_eq!(results.len(), 0);
    }

    #[test]
    fn test_audit_export() {
        let middleware = AuditLoggingMiddleware::new(AuditConfig::default());

        // Export should work even with no entries
        let json_export = middleware
            .export_logs(AuditExportFormat::Json, None)
            .unwrap();
        assert_eq!(json_export, "[]");

        let csv_export = middleware
            .export_logs(AuditExportFormat::Csv, None)
            .unwrap();
        assert!(csv_export.starts_with("entry_id,operation_id"));

        let xml_export = middleware
            .export_logs(AuditExportFormat::Xml, None)
            .unwrap();
        assert!(xml_export.contains("<audit_logs>"));
    }

    #[test]
    fn test_compliance_flags() {
        let config = AuditConfig {
            include_regulatory_flags: true,
            ..AuditConfig::default()
        };
        let middleware = AuditLoggingMiddleware::new(config);

        let effects = Effects::test(42);
        let account_id = aura_types::AccountId::new_with_effects(&effects);
        let device_id = aura_types::DeviceId::new_with_effects(&effects);

        let context = CryptoContext::new(
            account_id,
            device_id,
            "test".to_string(),
            SecurityLevel::Critical,
        );

        let operation = CryptoOperation::RotateKeys {
            old_threshold: 2,
            new_threshold: 3,
            new_participants: vec![device_id],
        };

        let flags = middleware.get_compliance_flags(&operation, &context);
        assert!(flags.contains(&"HIGH_SECURITY".to_string()));
        assert!(flags.contains(&"KEY_LIFECYCLE".to_string()));
        assert!(flags.contains(&"CRITICAL_OPERATION".to_string()));
        assert!(flags.contains(&"SOX_COMPLIANCE".to_string()));
        assert!(flags.contains(&"GDPR_RELEVANT".to_string()));
    }
}
