//! Configuration loading and merging utilities

use crate::AuraError;
use std::path::PathBuf;

/// Configuration source types with priorities
#[derive(Debug, Clone, PartialEq)]
pub enum ConfigSource {
    /// Configuration from file
    File {
        path: PathBuf,
        priority: ConfigPriority,
    },
    /// Configuration from environment variables
    Environment,
    /// Configuration from command line arguments
    CommandLine,
    /// Default configuration values
    Defaults,
}

/// Priority levels for configuration merging
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ConfigPriority {
    /// Low priority (typically defaults or system-wide configs)
    Low = 1,
    /// Medium priority (user configs, project files)
    Medium = 2, 
    /// High priority (explicit overrides, CLI args)
    High = 3,
}

impl ConfigSource {
    /// Get priority for a configuration source
    pub fn priority(&self) -> u8 {
        match self {
            ConfigSource::Defaults => 0,
            ConfigSource::File { priority, .. } => *priority as u8,
            ConfigSource::Environment => 10,
            ConfigSource::CommandLine => 20,
        }
    }
}

/// Configuration loader trait for different sources
pub trait ConfigLoader<T> {
    /// Error type for loading operations
    type Error: Into<AuraError>;
    
    /// Load configuration from a file path
    fn load_from_file(path: &std::path::Path) -> Result<T, Self::Error>;
    
    /// Load configuration from environment variables
    fn load_from_env() -> Result<T, Self::Error>;
    
    /// Merge this configuration with another configuration
    fn merge_with(&mut self, other: &T) -> Result<(), Self::Error>;
    
    /// Validate the configuration
    fn validate(&self) -> Result<(), Self::Error>;
    
    /// Set a configuration value from a string (for CLI parsing)
    fn set_from_string(&mut self, key: &str, value: &str) -> Result<(), Self::Error>;
    
    /// Handle positional arguments (for CLI parsing)
    fn handle_positional_arg(&mut self, arg: &str) -> Result<(), Self::Error>;
}

/// Default implementation for configuration loading
impl<T> ConfigLoader<T> for T
where
    T: serde::de::DeserializeOwned + Default + Clone,
    AuraError: From<toml::de::Error> + From<serde_json::Error>,
{
    type Error = AuraError;
    
    fn load_from_file(path: &std::path::Path) -> Result<T, Self::Error> {
        use std::fs;
        
        if !path.exists() {
            return Err(AuraError::not_found(format!(
                "Configuration file not found: {}", 
                path.display()
            )));
        }
        
        let content = fs::read_to_string(path)
            .map_err(|e| AuraError::internal(format!(
                "Failed to read config file {}: {}", 
                path.display(), e
            )))?;
        
        // Detect format by extension
        let config = match path.extension().and_then(|ext| ext.to_str()) {
            Some("toml") => {
                toml::from_str(&content)
                    .map_err(|e| AuraError::invalid(format!(
                        "Invalid TOML in {}: {}", 
                        path.display(), e
                    )))?
            }
            Some("json") => {
                serde_json::from_str(&content)
                    .map_err(|e| AuraError::invalid(format!(
                        "Invalid JSON in {}: {}", 
                        path.display(), e
                    )))?
            }
            _ => {
                return Err(AuraError::invalid(format!(
                    "Unsupported config file format: {}", 
                    path.display()
                )));
            }
        };
        
        Ok(config)
    }
    
    fn load_from_env() -> Result<T, Self::Error> {
        // Default implementation - just returns defaults
        // Real implementations would parse environment variables
        Ok(T::default())
    }
    
    fn merge_with(&mut self, _other: &T) -> Result<(), Self::Error> {
        // Default implementation - no merging
        // Real implementations would merge configuration fields
        Ok(())
    }
    
    fn validate(&self) -> Result<(), Self::Error> {
        // Default implementation - always valid
        // Real implementations would validate configuration constraints
        Ok(())
    }
    
    fn set_from_string(&mut self, _key: &str, _value: &str) -> Result<(), Self::Error> {
        // Default implementation - no-op
        // Real implementations would set configuration fields from strings
        Ok(())
    }
    
    fn handle_positional_arg(&mut self, _arg: &str) -> Result<(), Self::Error> {
        // Default implementation - ignore positional args
        Ok(())
    }
}

/// Configuration merger that handles hierarchical configuration merging
pub struct ConfigMerger<T> {
    configs: Vec<(T, ConfigSource)>,
}

impl<T> ConfigMerger<T> 
where
    T: ConfigLoader<T> + Clone + Default,
{
    /// Create a new configuration merger
    pub fn new() -> Self {
        Self {
            configs: Vec::new(),
        }
    }
    
    /// Add a configuration with its source
    pub fn add(mut self, config: T, source: ConfigSource) -> Self {
        self.configs.push((config, source));
        self
    }
    
    /// Merge all configurations according to priority
    pub fn merge(mut self) -> Result<T, T::Error> {
        if self.configs.is_empty() {
            return Ok(T::default());
        }
        
        // Sort by priority (lowest to highest)
        self.configs.sort_by_key(|(_, source)| ConfigSource::priority(source));
        
        // Start with the first (lowest priority) config
        let mut result = self.configs[0].0.clone();
        
        // Merge each subsequent config
        for (config, _) in self.configs.iter().skip(1) {
            result.merge_with(config)?;
        }
        
        result.validate()?;
        Ok(result)
    }
    
}

impl<T> Default for ConfigMerger<T> 
where
    T: ConfigLoader<T> + Clone + Default,
{
    fn default() -> Self {
        Self::new()
    }
}