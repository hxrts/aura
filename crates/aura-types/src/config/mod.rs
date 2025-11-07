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
//! use aura_types::config::{AuraConfig, ConfigLoader};
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

    /// Merge with CLI arguments (placeholder for future implementation)
    pub fn with_cli_args(self, _args: Vec<String>) -> Result<Self, AuraError> {
        // TODO: Implement CLI argument parsing and merging
        Ok(self)
    }

    /// Validate and build the final configuration
    pub fn build(self) -> Result<T, AuraError> {
        self.config.validate()?;
        Ok(self.config)
    }

    /// Check if the current config is just defaults
    fn is_defaults(&self) -> bool {
        // This is a simplified check - real implementation would compare with T::defaults()
        self.sources.is_empty()
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

impl<T: AuraConfig> ConfigWatcher<T> {
    /// Create a new configuration watcher
    pub fn new(config: T) -> Self {
        Self {
            current_config: config,
            file_path: None,
        }
    }

    /// Start watching a configuration file for changes
    pub fn watch_file(&mut self, path: &Path) -> Result<(), AuraError> {
        self.file_path = Some(path.to_path_buf());
        // TODO: Implement file watching using notify or similar
        Ok(())
    }

    /// Check for configuration changes and reload if necessary
    pub fn check_for_changes(&mut self) -> Result<bool, AuraError> {
        if let Some(_path) = &self.file_path {
            // TODO: Check file modification time and reload if changed
            // For now, always return false (no changes)
            Ok(false)
        } else {
            Ok(false)
        }
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
