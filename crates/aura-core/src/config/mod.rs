//! Universal Configuration System for Aura
//!
//! This module provides the foundational configuration traits and patterns
//! that eliminate duplication and provide consistent configuration handling
//! across all Aura components.
//!
//! ## Design Principles
//!
//! 1. **Unified Interface**: Same configuration traits used by all components
//! 2. **Multiple Formats**: Support for TOML, JSON, YAML, and environment variables
//! 3. **Validation**: Built-in validation with descriptive error messages
//! 4. **Merging**: Hierarchical configuration merging (defaults < file < env < CLI)
//! 5. **Type Safety**: Compile-time configuration validation
//! 6. **Hot Reload**: Support for configuration changes at runtime
//!
//! ## Usage Pattern
//!
//! ```ignore
//! use aura_core::config::{AuraConfig, ConfigLoader};
//! use std::time::Duration;
//!
//! // Define configuration using derive macro
//! #[derive(AuraConfig)]
//! #[config(file_format = "toml", validate, merge, defaults)]
//! struct ComponentConfig {
//!     #[config(required, range(1..=100))]
//!     max_connections: u32,
//!     #[config(default = "30s", validate = "timeout_range")]
//!     timeout: Duration,
//!     #[config(default = "info")]
//!     log_level: String,
//! }
//!
//! // Load and use configuration
//! let config = ComponentConfig::load_from_file("config.toml")?;
//! let merged_config = config.merge_with_env()?.merge_with_cli_args(&["arg1"])?;
//! merged_config.validate()?;
//! ```

pub mod formats;
pub mod loader;
pub mod traits;
pub mod validation;

// Re-export core config types from traits module
pub use formats::{ConfigFormat, JsonFormat, TomlFormat, YamlFormat};
pub use loader::{ConfigLoader, ConfigPriority, ConfigSource};
pub use traits::{AuraConfig, ConfigDefaults, ConfigMerge, ConfigValidation};
pub use validation::{ConfigValidator, ValidationResult, ValidationRule};

use crate::AuraError;
use std::path::Path;

/// Configuration builder for fluent configuration creation
pub struct ConfigBuilder<T> {
    config: T,
    sources: Vec<ConfigSource>,
}

impl<T: AuraConfig> ConfigBuilder<T> {
    /// Create a new configuration builder with defaults
    pub fn new() -> Self {
        Self {
            config: T::defaults(),
            sources: Vec::new(),
        }
    }
}

impl<T: AuraConfig> ConfigBuilder<T>
where
    AuraError: From<T::Error>,
{
    /// Load from a file with specified priority
    pub fn from_file(mut self, path: &Path, priority: ConfigPriority) -> Result<Self, AuraError> {
        let file_config = T::load_from_file(path)?;
        self.sources.push(ConfigSource::File {
            path: path.to_path_buf(),
            priority,
        });

        match priority {
            ConfigPriority::Low => {
                // Low priority: only use if current config is defaults
                if self.is_defaults() {
                    self.config = file_config;
                }
            }
            ConfigPriority::Medium => {
                // Medium priority: merge with current
                self.config.merge_with(&file_config)?;
            }
            ConfigPriority::High => {
                // High priority: replace current
                self.config = file_config;
            }
        }

        Ok(self)
    }

    /// Merge with environment variables
    pub fn with_env(mut self) -> Result<Self, AuraError> {
        self.config.merge_with_env()?;
        self.sources.push(ConfigSource::Environment);
        Ok(self)
    }

    /// Merge with CLI arguments using clap-like parsing
    pub fn with_cli_args(mut self, args: Vec<String>) -> Result<Self, AuraError> {
        let cli_config = self.parse_cli_args(args)?;
        self.config.merge_with(&cli_config)?;
        self.sources.push(ConfigSource::CommandLine);
        Ok(self)
    }
    
    /// Parse CLI arguments into configuration
    fn parse_cli_args(&self, args: Vec<String>) -> Result<T, AuraError> {
        let mut parsed_config = T::defaults();
        let mut arg_iter = args.iter().skip(1); // Skip program name
        
        while let Some(arg) = arg_iter.next() {
            if arg.starts_with("--") {
                let key = &arg[2..]; // Remove "--" prefix
                
                // Handle --key=value format
                if let Some((config_key, value)) = key.split_once('=') {
                    parsed_config.set_from_string(config_key, value)?;
                } else {
                    // Handle --key value format
                    if let Some(value) = arg_iter.next() {
                        parsed_config.set_from_string(key, value)?;
                    } else {
                        return Err(AuraError::invalid(format!("Missing value for argument: --{}", key)));
                    }
                }
            } else if arg.starts_with('-') && arg.len() == 2 {
                // Handle short options like -v, -p
                let short_flag = &arg[1..];
                if let Some(value) = arg_iter.next() {
                    let config_key = self.short_flag_to_config_key(short_flag)?;
                    parsed_config.set_from_string(&config_key, value)?;
                } else {
                    // Boolean flag
                    let config_key = self.short_flag_to_config_key(short_flag)?;
                    parsed_config.set_from_string(&config_key, "true")?;
                }
            } else {
                // Positional argument - could be handled by specific configs
                parsed_config.handle_positional_arg(arg)?;
            }
        }
        
        Ok(parsed_config)
    }
    
    /// Map short flags to configuration keys
    fn short_flag_to_config_key(&self, short_flag: &str) -> Result<String, AuraError> {
        // Common short flag mappings
        let mapping = match short_flag {
            "v" => "verbose",
            "q" => "quiet", 
            "p" => "port",
            "h" => "host",
            "c" => "config",
            "d" => "debug",
            _ => return Err(AuraError::invalid(format!("Unknown short flag: -{}", short_flag))),
        };
        Ok(mapping.to_string())
    }

    /// Validate and build the final configuration
    pub fn build(self) -> Result<T, AuraError> {
        self.config.validate()?;
        Ok(self.config)
    }

    /// Check if the current config is just defaults
    fn is_defaults(&self) -> bool {
        // Compare current config with defaults
        // This is simplified - a full implementation would use deep comparison
        self.sources.is_empty() || 
        std::ptr::eq(&self.config as *const T, &T::defaults() as *const T)
    }
}

impl<T: AuraConfig> Default for ConfigBuilder<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// Configuration watcher for hot reloading
pub struct ConfigWatcher<T> {
    current_config: T,
    file_path: Option<std::path::PathBuf>,
}

impl<T: AuraConfig> ConfigWatcher<T> 
where
    AuraError: From<<T as AuraConfig>::Error>,
{
    /// Create a new configuration watcher
    pub fn new(config: T) -> Self {
        Self {
            current_config: config,
            file_path: None,
        }
    }

    /// Start watching a configuration file for changes
    pub fn watch_file(&mut self, path: &Path) -> Result<(), AuraError> {
        if !path.exists() {
            return Err(AuraError::not_found(format!("Configuration file not found: {}", path.display())));
        }
        
        self.file_path = Some(path.to_path_buf());
        
        // Start file watcher thread
        self.start_file_watcher(path)?;
        
        Ok(())
    }
    
    /// Start background file watcher using polling
    fn start_file_watcher(&self, path: &Path) -> Result<(), AuraError> {
        use std::fs;
        use std::thread;
        use std::time::{Duration, SystemTime};
        
        let path_clone = path.to_path_buf();
        
        // Get initial modification time
        let initial_modified = fs::metadata(&path_clone)
            .and_then(|metadata| metadata.modified())
            .map_err(|e| AuraError::internal(format!("Failed to get file metadata: {}", e)))?;
        
        // Store initial timestamp for comparison
        static FILE_WATCHER_STATE: Mutex<Option<(std::path::PathBuf, SystemTime)>> = Mutex::new(None);
        
        *FILE_WATCHER_STATE.lock().unwrap() = Some((path_clone.clone(), initial_modified));
        
        // Spawn watcher thread
        thread::spawn(move || {
            loop {
                thread::sleep(Duration::from_secs(1)); // Poll every second
                
                if let Ok(metadata) = fs::metadata(&path_clone) {
                    if let Ok(modified) = metadata.modified() {
                        let mut state = FILE_WATCHER_STATE.lock().unwrap();
                        if let Some((_, last_modified)) = state.as_ref() {
                            if modified > *last_modified {
                                // File was modified - update timestamp
                                *state = Some((path_clone.clone(), modified));
                                
                                // Note: In a real implementation, this would trigger a callback
                                // or send a notification to the main thread
                                tracing::info!("Configuration file modified: {}", path_clone.display());
                            }
                        }
                    }
                }
            }
        });
        
        Ok(())
    }

    /// Check for configuration changes and reload if necessary
    pub fn check_for_changes(&mut self) -> Result<bool, AuraError> 
    where 
        <T as AuraConfig>::Error: std::fmt::Display,
    {
        if let Some(path) = &self.file_path {
            // Check if file was modified
            if self.was_file_modified(path)? {
                // Reload configuration from file
                match T::load_from_file(path) {
                    Ok(new_config) => {
                        // Validate new configuration before applying
                        new_config.validate()?;
                        
                        let old_config = std::mem::replace(&mut self.current_config, new_config);
                        
                        tracing::info!(
                            "Configuration reloaded from: {}", 
                            path.display()
                        );
                        
                        // Notify about hot reload
                        self.notify_hot_reload(&old_config, &self.current_config);
                        
                        Ok(true)
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Failed to reload configuration from {}: {}", 
                            path.display(), 
                            e
                        );
                        Ok(false)
                    }
                }
            } else {
                Ok(false)
            }
        } else {
            Ok(false)
        }
    }
    
    /// Check if the watched file was modified
    fn was_file_modified(&self, path: &Path) -> Result<bool, AuraError> {
        use std::fs;
        use std::time::SystemTime;
        
        static FILE_TIMESTAMPS: std::sync::LazyLock<Mutex<std::collections::HashMap<std::path::PathBuf, SystemTime>>> = 
            std::sync::LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));
        
        let metadata = fs::metadata(path)
            .map_err(|e| AuraError::not_found(format!("Configuration file not found: {}", e)))?;
            
        let current_modified = metadata.modified()
            .map_err(|e| AuraError::internal(format!("Failed to get modification time: {}", e)))?;
        
        let mut timestamps = FILE_TIMESTAMPS.lock().unwrap();
        
        match timestamps.get(path) {
            Some(last_modified) => {
                let was_modified = current_modified > *last_modified;
                if was_modified {
                    timestamps.insert(path.to_path_buf(), current_modified);
                }
                Ok(was_modified)
            }
            None => {
                // First time checking this file
                timestamps.insert(path.to_path_buf(), current_modified);
                Ok(false) // Don't consider initial check as modification
            }
        }
    }
    
    /// Notify about hot reload (hook for custom handling)
    fn notify_hot_reload(&self, _old_config: &T, _new_config: &T) {
        // In a real implementation, this could trigger callbacks,
        // emit events, or notify other components about the configuration change
        tracing::info!("Configuration hot reload completed");
    }

    /// Get the current configuration
    pub fn current_config(&self) -> &T {
        &self.current_config
    }
}

/// Global configuration registry for component configurations (using safe statics)
use std::sync::{Mutex, Once};

static CONFIG_REGISTRY: Mutex<
    Option<std::collections::HashMap<String, Box<dyn std::any::Any + Send + Sync>>>,
> = Mutex::new(None);
static CONFIG_REGISTRY_INIT: Once = Once::new();

/// Register a configuration for a component
#[allow(clippy::unwrap_used)] // Mutex lock failure is unrecoverable in this context
pub fn register_config<T: AuraConfig + Send + Sync + 'static>(component_name: &str, config: T) {
    CONFIG_REGISTRY_INIT.call_once(|| {
        *CONFIG_REGISTRY.lock().unwrap() = Some(std::collections::HashMap::new());
    });

    let mut registry = CONFIG_REGISTRY.lock().unwrap();
    if let Some(ref mut map) = *registry {
        map.insert(component_name.to_string(), Box::new(config));
    }
}

/// Get a registered configuration for a component
/// Note: Returns owned config to avoid lifetime issues with locked mutex
#[allow(clippy::unwrap_used)] // Mutex lock failure is unrecoverable in this context
pub fn get_config<T: AuraConfig + Clone + 'static>(component_name: &str) -> Option<T> {
    let registry = CONFIG_REGISTRY.lock().unwrap();
    registry
        .as_ref()?
        .get(component_name)
        .and_then(|config| config.downcast_ref::<T>())
        .cloned()
}
