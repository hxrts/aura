//! Backup coordination middleware for recovery protocol management

use super::{AgentContext, AgentHandler, AgentMiddleware};
use crate::error::Result;
use crate::middleware::AgentOperation;
use crate::utils::time::AgentTimeProvider;
use aura_types::AuraError;
use aura_types::DeviceId;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Duration;

/// Backup coordination middleware that manages backup and recovery operations
pub struct BackupCoordinationMiddleware {
    /// Backup coordinator
    coordinator: Arc<RwLock<BackupCoordinator>>,

    /// Configuration
    config: BackupConfig,

    /// Time provider for timestamp generation
    time_provider: Arc<AgentTimeProvider>,
}

impl BackupCoordinationMiddleware {
    /// Create new backup coordination middleware with production time provider
    pub fn new(config: BackupConfig) -> Self {
        Self {
            coordinator: Arc::new(RwLock::new(BackupCoordinator::new())),
            config,
            time_provider: Arc::new(AgentTimeProvider::production()),
        }
    }

    /// Create new backup coordination middleware with custom time provider
    pub fn with_time_provider(config: BackupConfig, time_provider: Arc<AgentTimeProvider>) -> Self {
        Self {
            coordinator: Arc::new(RwLock::new(BackupCoordinator::new())),
            config,
            time_provider,
        }
    }

    /// Get backup statistics
    pub fn stats(&self) -> BackupStats {
        let coordinator = self.coordinator.read().unwrap();
        coordinator.stats()
    }

    /// Cancel a backup operation
    pub fn cancel_backup(&self, backup_id: &str) -> Result<bool> {
        let mut coordinator = self.coordinator.write().map_err(|_| {
            AuraError::internal_error(
                "Failed to acquire write lock on backup coordinator".to_string(),
            )
        })?;

        let current_time = self.time_provider.timestamp_secs();
        Ok(coordinator.cancel_backup(backup_id, current_time))
    }

    /// Get backup status
    pub fn get_backup_status(&self, backup_id: &str) -> Result<Option<BackupStatus>> {
        let coordinator = self.coordinator.read().map_err(|_| {
            AuraError::internal_error(
                "Failed to acquire read lock on backup coordinator".to_string(),
            )
        })?;

        Ok(coordinator.get_backup_status(backup_id))
    }

    /// Clean up completed/failed backups
    pub fn cleanup_old_backups(&self) -> Result<usize> {
        let mut coordinator = self.coordinator.write().map_err(|_| {
            AuraError::internal_error(
                "Failed to acquire write lock on backup coordinator".to_string(),
            )
        })?;

        let current_time = self.time_provider.timestamp_secs();
        Ok(coordinator.cleanup_old_backups(self.config.backup_retention_period, current_time))
    }
}

impl AgentMiddleware for BackupCoordinationMiddleware {
    fn process(
        &self,
        operation: AgentOperation,
        context: &AgentContext,
        next: &dyn AgentHandler,
    ) -> Result<serde_json::Value> {
        match &operation {
            AgentOperation::InitiateBackup {
                backup_type,
                guardians,
            } => {
                // Clone the data we need for processing
                let backup_type_clone = backup_type.clone();
                let guardians_clone = guardians.clone();

                // Validate backup parameters
                self.validate_backup_parameters(&backup_type_clone, &guardians_clone)?;

                // Check backup limits
                self.check_backup_limits(&context.device_id)?;

                // Process backup initiation
                let result = next.handle(operation, context)?;

                // Track the backup if successful
                if result
                    .get("success")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
                {
                    if let Some(backup_id) = result.get("backup_id").and_then(|v| v.as_str()) {
                        self.track_backup(
                            backup_id.to_string(),
                            context.device_id.clone(),
                            backup_type_clone,
                            guardians_clone,
                        )?;
                    }
                }

                Ok(result)
            }

            _ => {
                // For other operations, just pass through
                next.handle(operation, context)
            }
        }
    }

    fn name(&self) -> &str {
        "backup_coordination"
    }
}

impl BackupCoordinationMiddleware {
    fn validate_backup_parameters(&self, backup_type: &str, guardians: &[String]) -> Result<()> {
        // Validate backup type
        if backup_type.is_empty() {
            return Err(AuraError::invalid_input(
                "Backup type cannot be empty".to_string(),
            ));
        }

        if backup_type.len() > self.config.max_backup_type_length {
            return Err(AuraError::invalid_input(format!(
                "Backup type too long: {} > {}",
                backup_type.len(),
                self.config.max_backup_type_length
            )));
        }

        // Validate allowed backup types
        if !self.config.allowed_backup_types.is_empty()
            && !self
                .config
                .allowed_backup_types
                .contains(&backup_type.to_string())
        {
            return Err(AuraError::invalid_input(format!(
                "Backup type '{}' not allowed",
                backup_type
            )));
        }

        // Validate guardians
        if guardians.is_empty() {
            return Err(AuraError::invalid_input(
                "Guardians list cannot be empty".to_string(),
            ));
        }

        if guardians.len() > self.config.max_guardians {
            return Err(AuraError::invalid_input(format!(
                "Too many guardians: {} > {}",
                guardians.len(),
                self.config.max_guardians
            )));
        }

        if guardians.len() < self.config.min_guardians {
            return Err(AuraError::invalid_input(format!(
                "Too few guardians: {} < {}",
                guardians.len(),
                self.config.min_guardians
            )));
        }

        // Check for duplicate guardians
        let mut unique_guardians = std::collections::HashSet::new();
        for guardian in guardians {
            if guardian.is_empty() {
                return Err(AuraError::invalid_input(
                    "Guardian ID cannot be empty".to_string(),
                ));
            }

            if guardian.len() > self.config.max_guardian_id_length {
                return Err(AuraError::invalid_input(format!(
                    "Guardian ID too long: {} > {}",
                    guardian.len(),
                    self.config.max_guardian_id_length
                )));
            }

            if !unique_guardians.insert(guardian.clone()) {
                return Err(AuraError::invalid_input(
                    "Duplicate guardians not allowed".to_string(),
                ));
            }
        }

        Ok(())
    }

    fn check_backup_limits(&self, device_id: &DeviceId) -> Result<()> {
        let coordinator = self.coordinator.read().map_err(|_| {
            AuraError::internal_error(
                "Failed to acquire read lock on backup coordinator".to_string(),
            )
        })?;

        let active_backups = coordinator.get_device_active_backups(device_id);

        if active_backups.len() >= self.config.max_concurrent_backups {
            return Err(AuraError::backup_limit_exceeded(format!(
                "Device has {} active backups, maximum is {}",
                active_backups.len(),
                self.config.max_concurrent_backups
            )));
        }

        // Check rate limiting
        let current_time = self.time_provider.timestamp_secs();
        let recent_backups = coordinator.get_device_recent_backups(
            device_id,
            self.config.rate_limit_window,
            current_time,
        );

        if recent_backups.len() >= self.config.max_backups_per_window {
            return Err(AuraError::backup_rate_limited(format!(
                "Too many backup attempts: {} in {} seconds",
                recent_backups.len(),
                self.config.rate_limit_window.as_secs()
            )));
        }

        Ok(())
    }

    fn track_backup(
        &self,
        backup_id: String,
        device_id: DeviceId,
        backup_type: String,
        guardians: Vec<String>,
    ) -> Result<()> {
        let mut coordinator = self.coordinator.write().map_err(|_| {
            AuraError::internal_error(
                "Failed to acquire write lock on backup coordinator".to_string(),
            )
        })?;

        let now = self.time_provider.timestamp_secs();
        let backup_operation = BackupOperation {
            backup_id: backup_id.clone(),
            backup_type,
            initiator: device_id,
            guardians,
            started_at: now,
            last_update: now,
            status: BackupStatus::InProgress,
            progress: 0,
            error_message: None,
        };

        coordinator.add_backup(backup_id, backup_operation);
        Ok(())
    }
}

/// Configuration for backup coordination middleware
#[derive(Debug, Clone)]
pub struct BackupConfig {
    /// Maximum concurrent backups per device
    pub max_concurrent_backups: usize,

    /// Rate limiting: max backups per window
    pub max_backups_per_window: usize,

    /// Rate limiting window duration
    pub rate_limit_window: Duration,

    /// Maximum guardians per backup
    pub max_guardians: usize,

    /// Minimum guardians per backup
    pub min_guardians: usize,

    /// Maximum backup type name length
    pub max_backup_type_length: usize,

    /// Maximum guardian ID length
    pub max_guardian_id_length: usize,

    /// Allowed backup types (empty = allow all)
    pub allowed_backup_types: Vec<String>,

    /// How long to keep completed/failed backup records
    pub backup_retention_period: Duration,

    /// Backup operation timeout
    pub backup_timeout: Duration,
}

impl Default for BackupConfig {
    fn default() -> Self {
        Self {
            max_concurrent_backups: 3,
            max_backups_per_window: 5,
            rate_limit_window: Duration::from_secs(3600), // 1 hour
            max_guardians: 10,
            min_guardians: 2,
            max_backup_type_length: 64,
            max_guardian_id_length: 128,
            allowed_backup_types: vec![
                "full".to_string(),
                "incremental".to_string(),
                "recovery".to_string(),
                "share_refresh".to_string(),
            ],
            backup_retention_period: Duration::from_secs(7 * 24 * 3600), // 7 days
            backup_timeout: Duration::from_secs(3600),                   // 1 hour
        }
    }
}

/// Backup operation information
#[derive(Debug, Clone)]
pub struct BackupOperation {
    pub backup_id: String,
    pub backup_type: String,
    pub initiator: DeviceId,
    pub guardians: Vec<String>,
    pub started_at: u64,
    pub last_update: u64,
    pub status: BackupStatus,
    pub progress: u8, // 0-100
    pub error_message: Option<String>,
}

/// Backup operation status
#[derive(Debug, Clone, PartialEq)]
pub enum BackupStatus {
    InProgress,
    Completed,
    Failed,
    Cancelled,
    TimedOut,
}

/// Backup coordinator for managing operations
struct BackupCoordinator {
    backups: HashMap<String, BackupOperation>,
    device_backups: HashMap<String, Vec<String>>, // device_id -> backup_ids
    total_backups_initiated: u64,
    total_backups_completed: u64,
    total_backups_failed: u64,
}

impl BackupCoordinator {
    fn new() -> Self {
        Self {
            backups: HashMap::new(),
            device_backups: HashMap::new(),
            total_backups_initiated: 0,
            total_backups_completed: 0,
            total_backups_failed: 0,
        }
    }

    fn add_backup(&mut self, backup_id: String, backup_operation: BackupOperation) {
        // Track backup
        self.backups
            .insert(backup_id.clone(), backup_operation.clone());
        self.total_backups_initiated += 1;

        // Track device backups
        let device_key = backup_operation.initiator.to_string();
        self.device_backups
            .entry(device_key)
            .or_insert_with(Vec::new)
            .push(backup_id);
    }

    fn cancel_backup(&mut self, backup_id: &str, current_time: u64) -> bool {
        if let Some(backup) = self.backups.get_mut(backup_id) {
            if backup.status == BackupStatus::InProgress {
                backup.status = BackupStatus::Cancelled;
                backup.last_update = current_time;
                return true;
            }
        }
        false
    }

    fn get_backup_status(&self, backup_id: &str) -> Option<BackupStatus> {
        self.backups.get(backup_id).map(|b| b.status.clone())
    }

    fn get_device_active_backups(&self, device_id: &DeviceId) -> Vec<&BackupOperation> {
        let device_key = device_id.to_string();
        self.device_backups
            .get(&device_key)
            .unwrap_or(&Vec::new())
            .iter()
            .filter_map(|backup_id| self.backups.get(backup_id))
            .filter(|backup| backup.status == BackupStatus::InProgress)
            .collect()
    }

    fn get_device_recent_backups(
        &self,
        device_id: &DeviceId,
        window: Duration,
        current_time: u64,
    ) -> Vec<&BackupOperation> {
        let device_key = device_id.to_string();
        let cutoff = current_time - window.as_secs();

        self.device_backups
            .get(&device_key)
            .unwrap_or(&Vec::new())
            .iter()
            .filter_map(|backup_id| self.backups.get(backup_id))
            .filter(|backup| backup.started_at > cutoff)
            .collect()
    }

    fn cleanup_old_backups(&mut self, retention_period: Duration, current_time: u64) -> usize {
        let cutoff = current_time - retention_period.as_secs();

        let mut to_remove = Vec::new();

        for (backup_id, backup) in &self.backups {
            if backup.last_update < cutoff
                && (backup.status == BackupStatus::Completed
                    || backup.status == BackupStatus::Failed
                    || backup.status == BackupStatus::Cancelled)
            {
                to_remove.push(backup_id.clone());
            }
        }

        let count = to_remove.len();

        for backup_id in to_remove {
            if let Some(backup) = self.backups.remove(&backup_id) {
                // Remove from device tracking
                let device_key = backup.initiator.to_string();
                if let Some(device_backups) = self.device_backups.get_mut(&device_key) {
                    device_backups.retain(|id| id != &backup_id);
                }
            }
        }

        count
    }

    fn stats(&self) -> BackupStats {
        let active_backups = self
            .backups
            .values()
            .filter(|b| b.status == BackupStatus::InProgress)
            .count();

        let completed_backups = self
            .backups
            .values()
            .filter(|b| b.status == BackupStatus::Completed)
            .count();

        let failed_backups = self
            .backups
            .values()
            .filter(|b| b.status == BackupStatus::Failed)
            .count();

        BackupStats {
            active_backups,
            total_backups: self.backups.len(),
            total_backups_initiated: self.total_backups_initiated,
            total_backups_completed: self.total_backups_completed,
            total_backups_failed: self.total_backups_failed,
            completed_backups,
            failed_backups,
        }
    }
}

/// Backup coordination statistics
#[derive(Debug, Clone)]
pub struct BackupStats {
    /// Number of active backups
    pub active_backups: usize,

    /// Total backup operations (all statuses)
    pub total_backups: usize,

    /// Total backups initiated
    pub total_backups_initiated: u64,

    /// Total backups completed
    pub total_backups_completed: u64,

    /// Total backups failed
    pub total_backups_failed: u64,

    /// Currently completed backups
    pub completed_backups: usize,

    /// Currently failed backups
    pub failed_backups: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::middleware::handler::NoOpHandler;
    use aura_crypto::Effects;
    use aura_types::{AccountIdExt, DeviceIdExt};

    #[test]
    fn test_backup_coordination_middleware() {
        let effects = Effects::test(42);
        let account_id = aura_types::AccountId::new_with_effects(&effects);
        let device_id = aura_types::DeviceId::new_with_effects(&effects);

        let middleware = BackupCoordinationMiddleware::new(BackupConfig::default());
        let handler = NoOpHandler;
        let context = AgentContext::new(account_id, device_id, "test".to_string());
        let operation = AgentOperation::InitiateBackup {
            backup_type: "full".to_string(),
            guardians: vec!["guardian1".to_string(), "guardian2".to_string()],
        };

        let result = middleware.process(operation, &context, &handler);
        assert!(result.is_ok());

        let stats = middleware.stats();
        assert_eq!(stats.total_backups_initiated, 0); // NoOpHandler doesn't provide backup_id for tracking
    }

    #[test]
    fn test_backup_validation() {
        let middleware = BackupCoordinationMiddleware::new(BackupConfig::default());

        // Valid backup
        assert!(middleware
            .validate_backup_parameters("full", &["guardian1".to_string(), "guardian2".to_string()])
            .is_ok());

        // Invalid backup type
        assert!(middleware
            .validate_backup_parameters("", &["guardian1".to_string(), "guardian2".to_string()])
            .is_err());

        assert!(middleware
            .validate_backup_parameters(
                "invalid-type",
                &["guardian1".to_string(), "guardian2".to_string()]
            )
            .is_err());

        // Invalid guardians
        assert!(middleware.validate_backup_parameters("full", &[]).is_err());
        assert!(middleware
            .validate_backup_parameters(
                "full",
                &["guardian1".to_string(), "guardian1".to_string()] // Duplicate
            )
            .is_err());
        assert!(middleware
            .validate_backup_parameters(
                "full",
                &["".to_string(), "guardian2".to_string()] // Empty guardian ID
            )
            .is_err());
    }

    #[test]
    fn test_backup_limits() {
        let effects = Effects::test(42);
        let account_id = aura_types::AccountId::new_with_effects(&effects);
        let device_id = aura_types::DeviceId::new_with_effects(&effects);

        let config = BackupConfig {
            max_concurrent_backups: 1, // Very low limit for testing
            ..BackupConfig::default()
        };
        let middleware = BackupCoordinationMiddleware::new(config);

        let context = AgentContext::new(account_id, device_id, "test".to_string());

        // Should succeed initially
        let result = middleware.check_backup_limits(&context.device_id);
        assert!(result.is_ok());
    }

    #[test]
    fn test_backup_cleanup() {
        let middleware = BackupCoordinationMiddleware::new(BackupConfig {
            backup_retention_period: Duration::from_secs(1),
            ..BackupConfig::default()
        });

        // Wait for retention period to pass
        std::thread::sleep(Duration::from_secs(2));

        let cleaned = middleware.cleanup_old_backups().unwrap();
        // Should be 0 since no backups were actually added
        assert_eq!(cleaned, 0);
    }
}
