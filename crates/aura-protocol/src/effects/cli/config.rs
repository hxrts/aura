//! CLI Configuration Types
//!
//! Configuration structures for CLI applications using the Aura effect system.

use std::path::PathBuf;

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

/// Output format options for CLI commands
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub enum OutputFormat {
    /// Human-readable output
    #[default]
    Human,
    /// JSON output for scripting
    Json,
}

