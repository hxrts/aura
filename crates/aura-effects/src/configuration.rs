//! Real configuration effects implementation
//!
//! Provides file-based configuration management with validation and backup support.

use async_trait::async_trait;
use aura_core::effects::agent::{
    ConfigError, ConfigValidationError, ConfigurationEffects, DeviceConfig,
};
use aura_core::AuraError;
use serde_json;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::fs;

/// Production configuration handler using file system storage
pub struct RealConfigurationHandler {
    config_path: PathBuf,
    backup_path: PathBuf,
}

impl RealConfigurationHandler {
    /// Create a new configuration handler
    pub fn new(config_dir: &Path) -> Self {
        Self {
            config_path: config_dir.join("device.json"),
            backup_path: config_dir.join("device.backup.json"),
        }
    }

    /// Load configuration from file or create default
    async fn load_or_default(&self) -> Result<DeviceConfig, ConfigError> {
        if self.config_path.exists() {
            let contents = fs::read_to_string(&self.config_path).await.map_err(|e| {
                ConfigError::StorageError(format!("Failed to read config file: {}", e))
            })?;

            serde_json::from_str(&contents)
                .map_err(|e| ConfigError::InvalidJson(format!("Failed to parse config: {}", e)))
        } else {
            // Create default config and save it
            let default_config = DeviceConfig::default();
            self.save_config(&default_config).await?;
            Ok(default_config)
        }
    }

    /// Save configuration to file
    async fn save_config(&self, config: &DeviceConfig) -> Result<(), ConfigError> {
        // Create parent directory if it doesn't exist
        if let Some(parent) = self.config_path.parent() {
            fs::create_dir_all(parent).await.map_err(|e| {
                ConfigError::StorageError(format!("Failed to create config directory: {}", e))
            })?;
        }

        let contents = serde_json::to_string_pretty(config).map_err(|e| {
            ConfigError::SerializationError(format!("Failed to serialize config: {}", e))
        })?;

        fs::write(&self.config_path, contents).await.map_err(|e| {
            ConfigError::StorageError(format!("Failed to write config file: {}", e))
        })?;

        Ok(())
    }

    /// Validate individual configuration values
    fn validate_device_config(&self, config: &DeviceConfig) -> Vec<ConfigValidationError> {
        let mut errors = vec![];

        // Validate device name
        if config.device_name.is_empty() {
            errors.push(ConfigValidationError {
                field: "device_name".to_string(),
                error: "Device name cannot be empty".to_string(),
                suggested_value: None,
            });
        }

        // Validate auto lock timeout (1 minute to 24 hours)
        if config.auto_lock_timeout < 60 || config.auto_lock_timeout > 86400 {
            errors.push(ConfigValidationError {
                field: "auto_lock_timeout".to_string(),
                error: "Auto lock timeout must be between 60 and 86400 seconds".to_string(),
                suggested_value: None,
            });
        }

        // Validate sync interval (5 minutes to 24 hours)
        if config.sync_interval < 300 || config.sync_interval > 86400 {
            errors.push(ConfigValidationError {
                field: "sync_interval".to_string(),
                error: "Sync interval must be between 300 and 86400 seconds".to_string(),
                suggested_value: None,
            });
        }

        // Validate max storage size (1MB minimum)
        if config.max_storage_size < 1024 * 1024 {
            errors.push(ConfigValidationError {
                field: "max_storage_size".to_string(),
                error: "Max storage size must be at least 1MB".to_string(),
                suggested_value: None,
            });
        }

        // Validate network timeout (1 second to 5 minutes)
        if config.network_timeout < 1000 || config.network_timeout > 300000 {
            errors.push(ConfigValidationError {
                field: "network_timeout".to_string(),
                error: "Network timeout must be between 1000 and 300000 milliseconds".to_string(),
                suggested_value: None,
            });
        }

        // Validate log level
        let valid_log_levels = ["TRACE", "DEBUG", "INFO", "WARN", "ERROR"];
        if !valid_log_levels.contains(&config.log_level.as_str()) {
            errors.push(ConfigValidationError {
                field: "log_level".to_string(),
                error: format!("Log level must be one of: {}", valid_log_levels.join(", ")),
                suggested_value: None,
            });
        }

        errors
    }
}

#[async_trait]
impl ConfigurationEffects for RealConfigurationHandler {
    async fn get_device_config(&self) -> Result<DeviceConfig, AuraError> {
        self.load_or_default()
            .await
            .map_err(|e| AuraError::invalid(e.to_string()))
    }

    async fn update_device_config(&self, config: &DeviceConfig) -> Result<(), AuraError> {
        // Validate before saving
        let validation_errors = self.validate_device_config(config);
        if !validation_errors.is_empty() {
            return Err(AuraError::invalid(format!(
                "Validation failed: {} errors",
                validation_errors.len()
            )));
        }

        // Create backup of current config
        if self.config_path.exists() {
            fs::copy(&self.config_path, &self.backup_path)
                .await
                .map_err(|e| AuraError::invalid(format!("Failed to create backup: {}", e)))?;
        }

        self.save_config(config)
            .await
            .map_err(|e| AuraError::invalid(e.to_string()))
    }

    async fn reset_to_defaults(&self) -> Result<(), AuraError> {
        let default_config = DeviceConfig::default();
        self.update_device_config(&default_config).await
    }

    async fn export_config(&self) -> Result<Vec<u8>, AuraError> {
        let config = self
            .load_or_default()
            .await
            .map_err(|e| AuraError::invalid(e.to_string()))?;
        let json = serde_json::to_vec_pretty(&config)
            .map_err(|e| AuraError::invalid(format!("Failed to serialize config: {}", e)))?;
        Ok(json)
    }

    async fn import_config(&self, config_data: &[u8]) -> Result<(), AuraError> {
        let config: DeviceConfig = serde_json::from_slice(config_data)
            .map_err(|e| AuraError::invalid(format!("Failed to parse config: {}", e)))?;

        self.update_device_config(&config).await
    }

    async fn validate_config(
        &self,
        config: &DeviceConfig,
    ) -> Result<Vec<ConfigValidationError>, AuraError> {
        Ok(self.validate_device_config(config))
    }

    async fn get_config_json(&self, key: &str) -> Result<Option<serde_json::Value>, AuraError> {
        let config = self
            .load_or_default()
            .await
            .map_err(|e| AuraError::invalid(e.to_string()))?;
        let config_json = serde_json::to_value(&config)
            .map_err(|e| AuraError::invalid(format!("Failed to convert config to JSON: {}", e)))?;

        if let Some(obj) = config_json.as_object() {
            Ok(obj.get(key).cloned())
        } else {
            Ok(None)
        }
    }

    async fn set_config_json(&self, key: &str, value: &serde_json::Value) -> Result<(), AuraError> {
        let mut config = self
            .load_or_default()
            .await
            .map_err(|e| AuraError::invalid(e.to_string()))?;
        let mut config_json = serde_json::to_value(&config)
            .map_err(|e| AuraError::invalid(format!("Failed to convert config to JSON: {}", e)))?;

        if let Some(obj) = config_json.as_object_mut() {
            obj.insert(key.to_string(), value.clone());
            config = serde_json::from_value(config_json).map_err(|e| {
                AuraError::invalid(format!("Failed to parse updated config: {}", e))
            })?;
            self.update_device_config(&config).await?;
        } else {
            return Err(AuraError::invalid("Configuration is not a valid object"));
        }

        Ok(())
    }

    async fn get_all_config(&self) -> Result<HashMap<String, serde_json::Value>, AuraError> {
        let config = self
            .load_or_default()
            .await
            .map_err(|e| AuraError::invalid(e.to_string()))?;
        let config_json = serde_json::to_value(&config)
            .map_err(|e| AuraError::invalid(format!("Failed to convert config to JSON: {}", e)))?;

        if let Some(obj) = config_json.as_object() {
            Ok(obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        } else {
            Ok(HashMap::new())
        }
    }
}
