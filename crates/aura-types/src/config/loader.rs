//! Configuration loading utilities

use crate::AuraError;
use std::path::PathBuf;

/// Configuration source priority levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ConfigPriority {
    /// Low priority: defaults and fallback configurations
    Low,
    /// Medium priority: file-based configurations
    Medium,
    /// High priority: environment variables and CLI arguments
    High,
}

/// Configuration source tracking
#[derive(Debug, Clone)]
pub enum ConfigSource {
    /// Configuration from default values
    Defaults,
    /// Configuration loaded from a file with given priority
    File {
        /// Path to the configuration file
        path: PathBuf,
        /// Priority level for this configuration source
        priority: ConfigPriority,
    },
    /// Configuration from environment variables
    Environment,
    /// Configuration from CLI arguments
    CliArgs(Vec<String>),
}

/// Configuration loader with source tracking
pub struct ConfigLoader<T> {
    /// The loaded configuration
    config: Option<T>,
    /// Track sources of configuration values
    sources: Vec<ConfigSource>,
}

impl<T> ConfigLoader<T> {
    /// Create a new configuration loader
    pub fn new() -> Self {
        Self {
            config: None,
            sources: Vec::new(),
        }
    }

    /// Load configuration with default values
    pub fn with_defaults(mut self, defaults: T) -> Self {
        self.config = Some(defaults);
        self.sources.push(ConfigSource::Defaults);
        self
    }

    /// Build the final configuration or return an error if none was provided
    pub fn build(self) -> Result<T, AuraError> {
        self.config
            .ok_or_else(|| AuraError::config_failed("No configuration provided"))
    }
}

impl<T> Default for ConfigLoader<T> {
    fn default() -> Self {
        Self::new()
    }
}
