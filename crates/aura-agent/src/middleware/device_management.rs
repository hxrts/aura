//! Device management middleware for secure storage and device operations

use super::{AgentContext, AgentHandler, AgentMiddleware};
use crate::device_secure_store::{SecureStorage, SecurityLevel};
use crate::error::Result;
use crate::middleware::AgentOperation;
use crate::utils::time::AgentTimeProvider;
use aura_types::AuraError;
use aura_types::DeviceId;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Device management middleware that handles secure storage operations
pub struct DeviceManagementMiddleware {
    /// Secure storage interface
    storage: Arc<dyn SecureStorage>,

    /// Device registry for tracking capabilities
    registry: Arc<RwLock<DeviceRegistry>>,

    /// Configuration
    config: DeviceConfig,

    /// Time provider for timestamp generation
    time_provider: Arc<AgentTimeProvider>,
}

impl DeviceManagementMiddleware {
    /// Create new device management middleware with production time provider
    pub fn new(storage: Arc<dyn SecureStorage>, config: DeviceConfig) -> Self {
        Self {
            storage,
            registry: Arc::new(RwLock::new(DeviceRegistry::new())),
            config,
            time_provider: Arc::new(AgentTimeProvider::production()),
        }
    }

    /// Create new device management middleware with custom time provider
    pub fn with_time_provider(
        storage: Arc<dyn SecureStorage>,
        config: DeviceConfig,
        time_provider: Arc<AgentTimeProvider>,
    ) -> Self {
        Self {
            storage,
            registry: Arc::new(RwLock::new(DeviceRegistry::new())),
            config,
            time_provider,
        }
    }

    /// Register a device with capabilities
    pub fn register_device(&self, device_id: DeviceId, capabilities: Vec<String>) -> Result<()> {
        let mut registry = self.registry.write().map_err(|_| {
            AuraError::internal_error("Failed to acquire write lock on device registry".to_string())
        })?;

        let now = self.time_provider.timestamp_secs();
        let device_info = DeviceInfo {
            device_id: device_id.clone(),
            capabilities,
            registered_at: now,
            last_seen: now,
            status: DeviceStatus::Active,
            storage_used: 0,
            successful_operations: 0,
            failed_operations: 0,
        };

        registry.register_device(device_id.to_string(), device_info);
        Ok(())
    }

    /// Get device management statistics
    pub fn stats(&self) -> DeviceStats {
        let registry = self.registry.read().unwrap();
        registry.stats()
    }
}

impl AgentMiddleware for DeviceManagementMiddleware {
    fn process(
        &self,
        operation: AgentOperation,
        context: &AgentContext,
        next: &dyn AgentHandler,
    ) -> Result<serde_json::Value> {
        // Update device last seen
        self.update_device_activity(&context.device_id)?;

        match &operation {
            AgentOperation::StoreData { data, capabilities } => {
                // Clone the data we need for processing
                let data_clone = data.clone();
                let capabilities_clone = capabilities.clone();

                // Validate storage request
                self.validate_storage_request(
                    &context.device_id,
                    &data_clone,
                    &capabilities_clone,
                )?;

                // Check device storage quota
                self.check_storage_quota(&context.device_id, data_clone.len())?;

                // Process storage operation
                let result = next.handle(operation, context)?;

                // Update storage statistics
                if result
                    .get("success")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
                {
                    self.update_storage_stats(&context.device_id, data_clone.len(), true)?;
                } else {
                    self.update_storage_stats(&context.device_id, 0, false)?;
                }

                Ok(result)
            }

            AgentOperation::RetrieveData {
                ref data_id,
                ref required_capability,
            } => {
                // Check device access to data
                self.validate_data_access(&context.device_id, data_id, required_capability)?;

                // Process retrieval operation
                let result = next.handle(operation, context)?;

                // Update access statistics
                if result
                    .get("success")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
                {
                    self.update_access_stats(&context.device_id, true)?;
                } else {
                    self.update_access_stats(&context.device_id, false)?;
                }

                Ok(result)
            }

            AgentOperation::Initialize { .. } => {
                // Ensure device is properly registered
                self.ensure_device_registered(&context.device_id)?;

                // Check device initialization capabilities
                self.validate_initialization_capabilities(&context.device_id)?;

                // Process initialization
                next.handle(operation, context)
            }

            _ => {
                // For other operations, just update activity and pass through
                next.handle(operation, context)
            }
        }
    }

    fn name(&self) -> &str {
        "device_management"
    }
}

impl DeviceManagementMiddleware {
    fn update_device_activity(&self, device_id: &DeviceId) -> Result<()> {
        let mut registry = self.registry.write().map_err(|_| {
            AuraError::internal_error("Failed to acquire write lock on device registry".to_string())
        })?;

        let now = self.time_provider.timestamp_secs();

        if let Some(device_info) = registry.devices.get_mut(&device_id.to_string()) {
            device_info.last_seen = now;
        }

        Ok(())
    }

    fn validate_storage_request(
        &self,
        device_id: &DeviceId,
        data: &[u8],
        capabilities: &[String],
    ) -> Result<()> {
        // Check data size limits
        if data.len() > self.config.max_data_size {
            return Err(AuraError::invalid_input(format!(
                "Data size {} exceeds maximum {}",
                data.len(),
                self.config.max_data_size
            )));
        }

        // Validate capabilities format
        for capability in capabilities {
            if capability.is_empty() {
                return Err(AuraError::invalid_input(
                    "Capability names cannot be empty".to_string(),
                ));
            }

            if capability.len() > self.config.max_capability_name_length {
                return Err(AuraError::invalid_input(format!(
                    "Capability name '{}' too long",
                    capability
                )));
            }
        }

        // Check device storage capabilities
        self.check_device_capability(device_id, "secure_storage")?;

        Ok(())
    }

    fn check_storage_quota(&self, device_id: &DeviceId, data_size: usize) -> Result<()> {
        let registry = self.registry.read().map_err(|_| {
            AuraError::internal_error("Failed to acquire read lock on device registry".to_string())
        })?;

        if let Some(device_info) = registry.devices.get(&device_id.to_string()) {
            let current_usage = device_info.storage_used;
            let new_usage = current_usage + data_size;

            if new_usage > self.config.max_storage_per_device {
                return Err(AuraError::storage_quota_exceeded(format!(
                    "Storage quota exceeded: {} + {} > {}",
                    current_usage, data_size, self.config.max_storage_per_device
                )));
            }
        }

        Ok(())
    }

    fn validate_data_access(
        &self,
        device_id: &DeviceId,
        _data_id: &str,
        required_capability: &str,
    ) -> Result<()> {
        // Check if device has the required capability
        self.check_device_capability(device_id, required_capability)?;

        // Additional access validation could be added here
        // (e.g., check ownership, time-based access, etc.)

        Ok(())
    }

    fn ensure_device_registered(&self, device_id: &DeviceId) -> Result<()> {
        let registry = self.registry.read().map_err(|_| {
            AuraError::internal_error("Failed to acquire read lock on device registry".to_string())
        })?;

        if !registry.devices.contains_key(&device_id.to_string()) {
            return Err(AuraError::device_not_registered(device_id.to_string()));
        }

        Ok(())
    }

    fn validate_initialization_capabilities(&self, device_id: &DeviceId) -> Result<()> {
        self.check_device_capability(device_id, "initialization")?;
        self.check_device_capability(device_id, "key_generation")?;
        Ok(())
    }

    fn check_device_capability(&self, device_id: &DeviceId, capability: &str) -> Result<()> {
        let registry = self.registry.read().map_err(|_| {
            AuraError::internal_error("Failed to acquire read lock on device registry".to_string())
        })?;

        if let Some(device_info) = registry.devices.get(&device_id.to_string()) {
            if !device_info.capabilities.contains(&capability.to_string()) {
                return Err(AuraError::capability_missing(format!(
                    "Device {} lacks required capability '{}'",
                    device_id, capability
                )));
            }
        } else {
            return Err(AuraError::device_not_registered(device_id.to_string()));
        }

        Ok(())
    }

    fn update_storage_stats(
        &self,
        device_id: &DeviceId,
        bytes_stored: usize,
        success: bool,
    ) -> Result<()> {
        let mut registry = self.registry.write().map_err(|_| {
            AuraError::internal_error("Failed to acquire write lock on device registry".to_string())
        })?;

        if let Some(device_info) = registry.devices.get_mut(&device_id.to_string()) {
            if success {
                device_info.storage_used += bytes_stored;
                device_info.successful_operations += 1;
            } else {
                device_info.failed_operations += 1;
            }
        }

        Ok(())
    }

    fn update_access_stats(&self, device_id: &DeviceId, success: bool) -> Result<()> {
        let mut registry = self.registry.write().map_err(|_| {
            AuraError::internal_error("Failed to acquire write lock on device registry".to_string())
        })?;

        if let Some(device_info) = registry.devices.get_mut(&device_id.to_string()) {
            if success {
                device_info.successful_operations += 1;
            } else {
                device_info.failed_operations += 1;
            }
        }

        Ok(())
    }
}

/// Configuration for device management middleware
#[derive(Debug, Clone)]
pub struct DeviceConfig {
    /// Maximum data size per operation
    pub max_data_size: usize,

    /// Maximum storage per device
    pub max_storage_per_device: usize,

    /// Maximum capability name length
    pub max_capability_name_length: usize,

    /// Whether to enforce device registration
    pub enforce_device_registration: bool,

    /// Default device capabilities
    pub default_device_capabilities: Vec<String>,

    /// Storage security level for device data
    pub device_storage_security_level: SecurityLevel,
}

impl Default for DeviceConfig {
    fn default() -> Self {
        Self {
            max_data_size: 10 * 1024 * 1024,           // 10 MB
            max_storage_per_device: 100 * 1024 * 1024, // 100 MB
            max_capability_name_length: 64,
            enforce_device_registration: true,
            default_device_capabilities: vec![
                "basic_operations".to_string(),
                "secure_storage".to_string(),
                "initialization".to_string(),
                "key_generation".to_string(),
            ],
            device_storage_security_level: SecurityLevel::HSM,
        }
    }
}

/// Device information tracking
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub device_id: DeviceId,
    pub capabilities: Vec<String>,
    pub registered_at: u64,
    pub last_seen: u64,
    pub status: DeviceStatus,
    pub storage_used: usize,
    pub successful_operations: u64,
    pub failed_operations: u64,
}

/// Device status
#[derive(Debug, Clone, PartialEq)]
pub enum DeviceStatus {
    Active,
    Inactive,
    Suspended,
    Revoked,
}

/// Device registry for tracking registered devices
struct DeviceRegistry {
    devices: HashMap<String, DeviceInfo>,
    total_registrations: u64,
    total_storage_used: usize,
}

impl DeviceRegistry {
    fn new() -> Self {
        Self {
            devices: HashMap::new(),
            total_registrations: 0,
            total_storage_used: 0,
        }
    }

    fn register_device(&mut self, device_key: String, device_info: DeviceInfo) {
        self.devices.insert(device_key, device_info);
        self.total_registrations += 1;
    }

    fn stats(&self) -> DeviceStats {
        let active_devices = self
            .devices
            .values()
            .filter(|d| d.status == DeviceStatus::Active)
            .count();

        let total_operations = self
            .devices
            .values()
            .map(|d| d.successful_operations + d.failed_operations)
            .sum();

        let successful_operations = self.devices.values().map(|d| d.successful_operations).sum();

        DeviceStats {
            total_devices: self.devices.len(),
            active_devices,
            total_registrations: self.total_registrations,
            total_storage_used: self.total_storage_used,
            total_operations,
            successful_operations,
        }
    }
}

/// Device management statistics
#[derive(Debug, Clone)]
pub struct DeviceStats {
    /// Total registered devices
    pub total_devices: usize,

    /// Active devices
    pub active_devices: usize,

    /// Total device registrations ever
    pub total_registrations: u64,

    /// Total storage used across all devices
    pub total_storage_used: usize,

    /// Total operations performed
    pub total_operations: u64,

    /// Successful operations
    pub successful_operations: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device_secure_store::memory::MemorySecureStorage;
    use crate::middleware::handler::NoOpHandler;
    use aura_crypto::Effects;
    use aura_types::{AccountIdExt, DeviceIdExt};

    #[test]
    fn test_device_management_middleware() {
        let effects = Effects::test(42);
        let account_id = aura_types::AccountId::new_with_effects(&effects);
        let device_id = aura_types::DeviceId::new_with_effects(&effects);

        let storage = Arc::new(MemorySecureStorage::new());
        let middleware = DeviceManagementMiddleware::new(storage, DeviceConfig::default());

        // Register device first
        middleware
            .register_device(
                device_id.clone(),
                vec!["secure_storage".to_string(), "basic_operations".to_string()],
            )
            .unwrap();

        let handler = NoOpHandler;
        let context = AgentContext::new(account_id, device_id, "test".to_string());
        let operation = AgentOperation::StoreData {
            data: vec![1, 2, 3, 4, 5],
            capabilities: vec!["read".to_string()],
        };

        let result = middleware.process(operation, &context, &handler);
        assert!(result.is_ok());

        let stats = middleware.stats();
        assert_eq!(stats.total_devices, 1);
        assert_eq!(stats.active_devices, 1);
    }

    #[test]
    fn test_storage_quota_enforcement() {
        let effects = Effects::test(42);
        let account_id = aura_types::AccountId::new_with_effects(&effects);
        let device_id = aura_types::DeviceId::new_with_effects(&effects);

        let storage = Arc::new(MemorySecureStorage::new());
        let config = DeviceConfig {
            max_storage_per_device: 100, // Very small quota for testing
            ..DeviceConfig::default()
        };
        let middleware = DeviceManagementMiddleware::new(storage, config);

        // Register device
        middleware
            .register_device(device_id.clone(), vec!["secure_storage".to_string()])
            .unwrap();

        let handler = NoOpHandler;
        let context = AgentContext::new(account_id, device_id, "test".to_string());
        let operation = AgentOperation::StoreData {
            data: vec![0u8; 200], // Exceeds quota
            capabilities: vec!["read".to_string()],
        };

        let result = middleware.process(operation, &context, &handler);
        assert!(result.is_err());
    }

    #[test]
    fn test_capability_validation() {
        let effects = Effects::test(42);
        let account_id = aura_types::AccountId::new_with_effects(&effects);
        let device_id = aura_types::DeviceId::new_with_effects(&effects);

        let storage = Arc::new(MemorySecureStorage::new());
        let middleware = DeviceManagementMiddleware::new(storage, DeviceConfig::default());

        // Register device without secure_storage capability
        middleware
            .register_device(device_id.clone(), vec!["basic_operations".to_string()])
            .unwrap();

        let handler = NoOpHandler;
        let context = AgentContext::new(account_id, device_id, "test".to_string());
        let operation = AgentOperation::StoreData {
            data: vec![1, 2, 3],
            capabilities: vec!["read".to_string()],
        };

        let result = middleware.process(operation, &context, &handler);
        assert!(result.is_err()); // Should fail due to missing capability
    }
}
