//! CLI Effect Traits
//!
//! CLI-specific effect traits that compose core effects for command-line operations.
//! These effects follow the unified effect system architecture.

use async_trait::async_trait;
use std::path::{Path, PathBuf};

pub mod cli;
pub mod config;
pub mod output;

pub use cli::*;
pub use config::*;
pub use output::*;

/// CLI-specific effects for command-line operations
/// Composes core effects into CLI-focused capabilities
#[async_trait]
pub trait CliEffects: Send + Sync {
    /// Log an informational message
    async fn log_info(&self, message: &str);

    /// Log a warning message
    async fn log_warning(&self, message: &str);

    /// Log an error message
    async fn log_error(&self, message: &str);

    /// Create a directory and all parent directories
    async fn create_dir_all(&self, path: &Path) -> crate::effects::Result<()>;

    /// Write content to a file
    async fn write_file(&self, path: &Path, content: &[u8]) -> crate::effects::Result<()>;

    /// Read content from a file
    async fn read_file(&self, path: &Path) -> crate::effects::Result<Vec<u8>>;

    /// Check if a file exists
    async fn file_exists(&self, path: &Path) -> bool;

    /// Format output for display
    async fn format_output(&self, data: &str) -> String;

    /// Get current timestamp for operations
    async fn current_timestamp(&self) -> u64;
}

/// Configuration management effects
#[async_trait]
pub trait ConfigEffects: Send + Sync {
    /// Load configuration from file
    async fn load_config(&self, path: &Path) -> crate::effects::Result<CliConfig>;

    /// Save configuration to file
    async fn save_config(&self, path: &Path, config: &CliConfig) -> crate::effects::Result<()>;

    /// Validate configuration structure
    async fn validate_config(&self, config: &CliConfig) -> crate::effects::Result<()>;

    /// Get default configuration directory
    async fn default_config_dir(&self) -> PathBuf;
}

/// Output formatting and display effects
#[async_trait]
pub trait OutputEffects: Send + Sync {
    /// Display formatted output to user
    async fn display(&self, content: &str);

    /// Display error message to user
    async fn display_error(&self, error: &str);

    /// Display success message to user
    async fn display_success(&self, message: &str);

    /// Display progress information
    async fn display_progress(&self, message: &str, progress: f64);

    /// Format data as JSON
    async fn format_json(&self, data: &serde_json::Value) -> crate::effects::Result<String>;

    /// Format data as human-readable text
    async fn format_text(&self, data: &str) -> String;
}

/// CLI configuration structure
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CliConfig {
    /// Default device ID
    pub device_id: Option<String>,

    /// Default threshold for operations
    pub threshold: Option<u32>,

    /// Default number of devices
    pub num_devices: Option<u32>,

    /// Default output directory
    pub output_dir: Option<PathBuf>,

    /// Logging configuration
    pub logging: LoggingConfig,

    /// Network configuration
    pub network: NetworkConfig,
}

/// Logging configuration
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LoggingConfig {
    /// Log level (debug, info, warn, error)
    pub level: String,

    /// Enable structured logging
    pub structured: bool,

    /// Log file path
    pub file: Option<PathBuf>,
}

/// Network configuration
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NetworkConfig {
    /// Default port for node operations
    pub default_port: u16,

    /// Connection timeout in seconds
    pub timeout: u64,

    /// Maximum number of retries
    pub max_retries: u32,
}

impl Default for CliConfig {
    fn default() -> Self {
        Self {
            device_id: None,
            threshold: Some(2),
            num_devices: Some(3),
            output_dir: Some(PathBuf::from(".aura")),
            logging: LoggingConfig {
                level: "info".to_string(),
                structured: false,
                file: None,
            },
            network: NetworkConfig {
                default_port: 58835,
                timeout: 30,
                max_retries: 3,
            },
        }
    }
}
