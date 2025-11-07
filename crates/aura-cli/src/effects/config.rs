//! Configuration Effect Implementations

use super::{ConfigEffects, CliConfig};
use async_trait::async_trait;
use anyhow::Result;
use std::path::PathBuf;

/// Configuration effect handler
pub struct ConfigEffectHandler<E> {
    inner: E,
}

impl<E> ConfigEffectHandler<E> {
    pub fn new(inner: E) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl<E> ConfigEffects for ConfigEffectHandler<E>
where
    E: aura_protocol::StorageEffects + 
       aura_protocol::ConsoleEffects + 
       Send + Sync,
{
    async fn load_config(&self, path: &PathBuf) -> Result<CliConfig> {
        let path_str = path.display().to_string();
        
        match self.inner.retrieve(&path_str).await {
            Ok(Some(data)) => {
                let config_str = String::from_utf8(data)
                    .map_err(|e| anyhow::anyhow!("Invalid UTF-8 in config file: {}", e))?;
                
                let config: CliConfig = toml::from_str(&config_str)
                    .map_err(|e| anyhow::anyhow!("Failed to parse config: {}", e))?;
                
                self.validate_config(&config).await?;
                Ok(config)
            }
            Ok(None) | Err(_) => {
                self.inner.log_info(&format!("Config file not found: {}, using defaults", path.display()), &[]);
                Ok(CliConfig::default())
            }
        }
    }
    
    async fn save_config(&self, path: &PathBuf, config: &CliConfig) -> Result<()> {
        self.validate_config(config).await?;
        
        let config_toml = toml::to_string(config)
            .map_err(|e| anyhow::anyhow!("Failed to serialize config: {}", e))?;
        
        let path_str = path.display().to_string();
        self.inner.store(&path_str, config_toml.as_bytes().to_vec()).await
            .map_err(|e| anyhow::anyhow!("Failed to save config: {}", e))?;
        
        self.inner.log_info(&format!("Configuration saved to {}", path.display()), &[]);
        Ok(())
    }
    
    async fn validate_config(&self, config: &CliConfig) -> Result<()> {
        // Validate threshold settings
        if let (Some(threshold), Some(num_devices)) = (config.threshold, config.num_devices) {
            if threshold > num_devices {
                return Err(anyhow::anyhow!(
                    "Threshold ({}) cannot be greater than number of devices ({})",
                    threshold, num_devices
                ));
            }
            if threshold == 0 {
                return Err(anyhow::anyhow!("Threshold must be greater than 0"));
            }
        }
        
        // Validate log level
        match config.logging.level.as_str() {
            "trace" | "debug" | "info" | "warn" | "error" => {},
            _ => return Err(anyhow::anyhow!("Invalid log level: {}", config.logging.level)),
        }
        
        // Validate network settings
        if config.network.timeout == 0 {
            return Err(anyhow::anyhow!("Network timeout must be greater than 0"));
        }
        
        Ok(())
    }
    
    async fn default_config_dir(&self) -> PathBuf {
        // Use a standard default directory
        PathBuf::from(".aura")
    }
}