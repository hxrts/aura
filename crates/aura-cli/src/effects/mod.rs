//! Layer 7: CLI Effect Traits - Command-Line Operation Composition
//!
//! CLI-specific effect traits for command-line operations.
//! Located in aura-cli (Layer 7) because these are UI-specific abstractions.
//!
//! **Effect Composition**:
//! - **CliEffects**: Base CLI operations (logging, file I/O, formatting, timestamps)
//! - **ConfigEffects**: Configuration management (load, save, validate)
//! - **OutputEffects**: Display formatting (JSON, text, progress, colors)
//!
//! These traits compose core infrastructure effects (ConsoleEffects, StorageEffects,
//! PhysicalTimeEffects) with CLI-specific formatting and display logic.

use async_trait::async_trait;
use aura_core::AuraResult;
use std::path::{Path, PathBuf};

pub mod config;
pub mod handler;
pub mod output;

pub use config::*;
pub use handler::*;
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
    async fn create_dir_all(&self, path: &Path) -> AuraResult<()>;

    /// Write content to a file
    async fn write_file(&self, path: &Path, content: &[u8]) -> AuraResult<()>;

    /// Read content from a file
    async fn read_file(&self, path: &Path) -> AuraResult<Vec<u8>>;

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
    async fn load_config(&self, path: &Path) -> AuraResult<CliConfig>;

    /// Save configuration to file
    async fn save_config(&self, path: &Path, config: &CliConfig) -> AuraResult<()>;

    /// Validate configuration structure
    async fn validate_config(&self, config: &CliConfig) -> AuraResult<()>;

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
    async fn format_json(&self, data: &serde_json::Value) -> AuraResult<String>;

    /// Format data as human-readable text
    async fn format_text(&self, data: &str) -> String;
}
