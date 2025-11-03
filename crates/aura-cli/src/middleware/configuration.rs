//! Configuration middleware for CLI operations

use super::{CliMiddleware, CliHandler, CliOperation, CliContext, CliConfig};
use crate::CliError;
use serde_json::{json, Value};
use std::sync::{Arc, RwLock};

/// Middleware for configuration loading, validation, and management
pub struct ConfigurationMiddleware {
    /// Cached configuration
    config_cache: Arc<RwLock<Option<CliConfig>>>,
    /// Auto-load configuration
    auto_load: bool,
    /// Configuration file paths to try
    config_paths: Vec<std::path::PathBuf>,
}

impl ConfigurationMiddleware {
    /// Create new configuration middleware
    pub fn new() -> Self {
        Self {
            config_cache: Arc::new(RwLock::new(None)),
            auto_load: true,
            config_paths: vec![
                dirs::home_dir().unwrap_or_default().join(".aura").join("config.toml"),
                std::env::current_dir().unwrap_or_default().join("aura.toml"),
            ],
        }
    }
    
    /// Enable or disable auto-loading
    pub fn with_auto_load(mut self, auto_load: bool) -> Self {
        self.auto_load = auto_load;
        self
    }
    
    /// Add configuration file path
    pub fn with_config_path(mut self, path: std::path::PathBuf) -> Self {
        self.config_paths.push(path);
        self
    }
    
    /// Load configuration from file
    fn load_config(&self, path: &std::path::Path) -> Result<CliConfig, CliError> {
        if !path.exists() {
            return Ok(CliConfig::default());
        }
        
        let config_str = std::fs::read_to_string(path).map_err(|e| {
            CliError::FileSystem(format!("Failed to read config file {}: {}", path.display(), e))
        })?;
        
        let config: CliConfig = toml::from_str(&config_str).map_err(|e| {
            CliError::Configuration(format!("Failed to parse config file {}: {}", path.display(), e))
        })?;
        
        Ok(config)
    }
    
    /// Save configuration to file
    fn save_config(&self, config: &CliConfig, path: &std::path::Path) -> Result<(), CliError> {
        // Create directory if needed
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                CliError::FileSystem(format!("Failed to create config directory: {}", e))
            })?;
        }
        
        let config_str = toml::to_string_pretty(config).map_err(|e| {
            CliError::Serialization(format!("Failed to serialize config: {}", e))
        })?;
        
        std::fs::write(path, config_str).map_err(|e| {
            CliError::FileSystem(format!("Failed to write config file: {}", e))
        })?;
        
        Ok(())
    }
    
    /// Get cached configuration or load it
    fn get_config(&self) -> Result<CliConfig, CliError> {
        // Try to get from cache first
        {
            let cache = self.config_cache.read().map_err(|_| {
                CliError::OperationFailed("Failed to acquire config cache lock".to_string())
            })?;
            
            if let Some(config) = cache.as_ref() {
                return Ok(config.clone());
            }
        }
        
        // Load from file
        let mut config = CliConfig::default();
        
        for path in &self.config_paths {
            if path.exists() {
                config = self.load_config(path)?;
                break;
            }
        }
        
        // Cache the configuration
        {
            let mut cache = self.config_cache.write().map_err(|_| {
                CliError::OperationFailed("Failed to acquire config cache lock".to_string())
            })?;
            *cache = Some(config.clone());
        }
        
        Ok(config)
    }
    
    /// Update cached configuration
    fn update_config(&self, config: CliConfig) -> Result<(), CliError> {
        let mut cache = self.config_cache.write().map_err(|_| {
            CliError::OperationFailed("Failed to acquire config cache lock".to_string())
        })?;
        *cache = Some(config);
        Ok(())
    }
    
    /// Validate configuration
    fn validate_config(&self, config: &CliConfig) -> Result<(), CliError> {
        // Basic validation
        if config.timeout_seconds == 0 {
            return Err(CliError::Configuration(
                "Timeout cannot be zero".to_string()
            ));
        }
        
        if config.timeout_seconds > 3600 {
            return Err(CliError::Configuration(
                "Timeout cannot exceed 1 hour".to_string()
            ));
        }
        
        // Validate config path is writable
        if let Some(parent) = config.config_path.parent() {
            if parent.exists() && !parent.is_dir() {
                return Err(CliError::Configuration(
                    format!("Config path parent is not a directory: {}", parent.display())
                ));
            }
        }
        
        Ok(())
    }
}

impl Default for ConfigurationMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

impl CliMiddleware for ConfigurationMiddleware {
    fn process(
        &self,
        operation: CliOperation,
        context: &CliContext,
        next: &dyn CliHandler,
    ) -> Result<Value, CliError> {
        match operation {
            CliOperation::LoadConfig { config_path } => {
                // Load specific configuration file
                let config = self.load_config(&config_path)?;
                self.validate_config(&config)?;
                self.update_config(config.clone())?;
                
                Ok(json!({
                    "status": "success",
                    "message": "Configuration loaded",
                    "config_path": config_path.display().to_string(),
                    "config": config
                }))
            }
            
            CliOperation::Init { config_path, force } => {
                // Initialize configuration
                let config = CliConfig::default();
                
                if config_path.exists() && !force {
                    return Ok(json!({
                        "status": "error",
                        "message": "Configuration file already exists. Use --force to overwrite."
                    }));
                }
                
                self.save_config(&config, &config_path)?;
                self.update_config(config.clone())?;
                
                Ok(json!({
                    "status": "success",
                    "message": "Configuration initialized",
                    "config_path": config_path.display().to_string(),
                    "config": config
                }))
            }
            
            _ => {
                // For other operations, ensure config is loaded if auto_load is enabled
                if self.auto_load {
                    let config = self.get_config()?;
                    
                    // Update context with loaded config
                    let mut enhanced_context = context.clone();
                    enhanced_context.config = config;
                    
                    next.handle(operation, &enhanced_context)
                } else {
                    next.handle(operation, context)
                }
            }
        }
    }
    
    fn name(&self) -> &str {
        "configuration"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::middleware::handler::NoOpCliHandler;
    use tempfile::tempdir;
    
    #[test]
    fn test_config_loading() {
        let middleware = ConfigurationMiddleware::new();
        let handler = NoOpCliHandler;
        let context = CliContext::new("test".to_string(), vec![]);
        
        // Test loading default config (should not fail)
        let result = middleware.process(
            CliOperation::Command { args: vec!["test".to_string()] },
            &context,
            &handler,
        );
        
        assert!(result.is_ok());
    }
    
    #[test]
    fn test_config_initialization() {
        let middleware = ConfigurationMiddleware::new();
        let handler = NoOpCliHandler;
        let context = CliContext::new("init".to_string(), vec![]);
        
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("test_config.toml");
        
        let result = middleware.process(
            CliOperation::Init { 
                config_path: config_path.clone(),
                force: false 
            },
            &context,
            &handler,
        );
        
        assert!(result.is_ok());
        assert!(config_path.exists());
        
        let value = result.unwrap();
        assert_eq!(value["status"], "success");
    }
    
    #[test]
    fn test_config_validation() {
        let middleware = ConfigurationMiddleware::new();
        
        let mut invalid_config = CliConfig::default();
        invalid_config.timeout_seconds = 0;
        
        let result = middleware.validate_config(&invalid_config);
        assert!(result.is_err());
    }
}