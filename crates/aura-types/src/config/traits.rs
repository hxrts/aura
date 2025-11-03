//! Core configuration traits extracted from analysis

use crate::AuraError;

/// Core configuration trait that all Aura configurations must implement
pub trait AuraConfig:
    ConfigValidation + ConfigMerge<Self> + ConfigDefaults + Clone + Send + Sync
{
    /// Error type returned by configuration operations
    type Error: std::error::Error + Send + Sync + 'static;

    /// Load configuration from a file
    fn load_from_file(path: &std::path::Path) -> Result<Self, Self::Error>
    where
        Self: Sized;

    /// Save configuration to a file
    fn save_to_file(&self, path: &std::path::Path) -> Result<(), Self::Error>;

    /// Merge with environment variables
    fn merge_with_env(&mut self) -> Result<(), Self::Error>;
}

/// Configuration validation trait
pub trait ConfigValidation {
    /// Validate the configuration
    fn validate(&self) -> Result<(), AuraError>;
}

/// Configuration merging trait
pub trait ConfigMerge<T> {
    /// Merge another configuration into this one
    fn merge_with(&mut self, other: &T) -> Result<(), AuraError>;
}

/// Configuration defaults trait
pub trait ConfigDefaults {
    /// Create configuration with default values
    fn defaults() -> Self;
}
