//! CLI middleware system
//!
//! This module implements the algebraic effect-style middleware pattern for CLI operations.
//! All command processing, input validation, output formatting, and error handling functionality
//! is implemented as composable middleware layers that can be stacked and configured.

pub mod authentication;
pub mod configuration;
pub mod error_handling;
pub mod handler;
pub mod input_validation;
pub mod output_formatting;
pub mod progress_reporting;
pub mod stack;

// Re-export core middleware types
pub use aura_protocol::middleware::{MiddlewareContext, MiddlewareResult};
pub use handler::{CliHandler, CliOperation, CliResult};
pub use stack::{CliMiddlewareStack, CliStackBuilder};

// Re-export middleware implementations
pub use authentication::AuthenticationMiddleware;
pub use configuration::ConfigurationMiddleware;
pub use error_handling::ErrorHandlingMiddleware;
pub use input_validation::InputValidationMiddleware;
pub use output_formatting::OutputFormattingMiddleware;
pub use progress_reporting::ProgressReportingMiddleware;

use aura_types::{AccountId, DeviceId};
use serde::{Deserialize, Serialize};

/// Context for CLI middleware operations
#[derive(Debug, Clone)]
pub struct CliContext {
    /// Command being executed
    pub command: String,
    /// Arguments provided to the command
    pub args: Vec<String>,
    /// Current working directory
    pub working_dir: std::path::PathBuf,
    /// Environment variables
    pub env: std::collections::HashMap<String, String>,
    /// User configuration
    pub config: CliConfig,
    /// Interactive mode flag
    pub interactive: bool,
    /// Verbose output flag
    pub verbose: bool,
    /// Request timestamp
    pub timestamp: u64,
    /// Additional metadata
    pub metadata: std::collections::HashMap<String, String>,
}

/// CLI configuration structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliConfig {
    /// Configuration file path
    pub config_path: std::path::PathBuf,
    /// Default account ID
    pub default_account: Option<AccountId>,
    /// Default device ID
    pub default_device: Option<DeviceId>,
    /// Output format preference
    pub output_format: OutputFormat,
    /// Color output preference
    pub color_output: bool,
    /// Progress reporting preference
    pub show_progress: bool,
    /// Log level
    pub log_level: LogLevel,
    /// Timeout settings
    pub timeout_seconds: u64,
}

/// Output format options
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OutputFormat {
    /// Human-readable text output
    Text,
    /// JSON output
    Json,
    /// YAML output
    Yaml,
    /// Table format
    Table,
    /// CSV format
    Csv,
}

/// Log level configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LogLevel {
    /// Only errors
    Error,
    /// Warnings and errors
    Warn,
    /// Info, warnings, and errors
    Info,
    /// Debug information
    Debug,
    /// All output including trace
    Trace,
}

impl CliContext {
    /// Create a new CLI context
    pub fn new(command: String, args: Vec<String>) -> Self {
        let working_dir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("/"));
        let env = std::env::vars().collect();

        Self {
            command,
            args,
            working_dir,
            env,
            config: CliConfig::default(),
            interactive: atty::is(atty::Stream::Stdin),
            verbose: false,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            metadata: std::collections::HashMap::new(),
        }
    }

    /// Add metadata to the context
    pub fn with_metadata(mut self, key: String, value: String) -> Self {
        self.metadata.insert(key, value);
        self
    }

    /// Set verbosity
    pub fn with_verbose(mut self, verbose: bool) -> Self {
        self.verbose = verbose;
        self
    }

    /// Set configuration
    pub fn with_config(mut self, config: CliConfig) -> Self {
        self.config = config;
        self
    }
}

impl Default for CliConfig {
    fn default() -> Self {
        Self {
            config_path: dirs::home_dir()
                .unwrap_or_else(|| std::path::PathBuf::from("/"))
                .join(".aura")
                .join("config.toml"),
            default_account: None,
            default_device: None,
            output_format: OutputFormat::Text,
            color_output: true,
            show_progress: true,
            log_level: LogLevel::Info,
            timeout_seconds: 30,
        }
    }
}

impl Default for OutputFormat {
    fn default() -> Self {
        OutputFormat::Text
    }
}

impl Default for LogLevel {
    fn default() -> Self {
        LogLevel::Info
    }
}
