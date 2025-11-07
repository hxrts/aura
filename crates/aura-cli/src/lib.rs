//! Aura CLI Library
//!
//! This library provides the command-line interface for the Aura threshold identity platform.
//! It uses the unified effect system architecture for all operations following the guidance
//! from docs/400_effect_system.md.
//!
//! The aura-cli crate adheres to the unified effect system by:
//! - Using AuraEffectSystem for all system interactions
//! - Implementing CLI-specific effects as composition of core effects
//! - Following proper dependency injection patterns
//! - Avoiding direct system access except in effect handlers

#![allow(missing_docs)]

pub mod effects;
pub mod handlers;

// Re-export key types and traits
pub use effects::{CliConfig, CliEffects, ConfigEffects, OutputEffects};
pub use handlers::CliHandler;

use aura_protocol::AuraEffectSystem;
use aura_types::identifiers::DeviceId;

/// Create a CLI handler for the given device ID
pub fn create_cli_handler(device_id: DeviceId) -> CliHandler {
    let effect_system = AuraEffectSystem::for_production(device_id);
    CliHandler::new(effect_system)
}

/// Create a test CLI handler for the given device ID
pub fn create_test_cli_handler(device_id: DeviceId) -> CliHandler {
    let effect_system = AuraEffectSystem::for_testing(device_id);
    CliHandler::new(effect_system)
}

/// Create a CLI handler with a generated device ID
pub fn create_default_cli_handler() -> CliHandler {
    let device_id = DeviceId::new();
    create_cli_handler(device_id)
}

/// Create a test CLI handler with a generated device ID
pub fn create_default_test_cli_handler() -> CliHandler {
    let device_id = DeviceId::new();
    create_test_cli_handler(device_id)
}

/// Scenario action types
#[derive(Debug, Clone, clap::Subcommand)]
pub enum ScenarioAction {
    /// Discover scenarios in a directory tree
    Discover {
        /// Root directory to search
        #[arg(long)]
        root: std::path::PathBuf,
        /// Whether to validate discovered scenarios
        #[arg(long)]
        validate: bool,
    },
    /// List available scenarios
    List {
        /// Directory containing scenarios
        #[arg(long)]
        directory: std::path::PathBuf,
        /// Show detailed information
        #[arg(long)]
        detailed: bool,
    },
    /// Validate scenario configurations
    Validate {
        /// Directory containing scenarios
        #[arg(long)]
        directory: std::path::PathBuf,
        /// Validation strictness level
        #[arg(long)]
        strictness: Option<String>,
    },
    /// Run scenarios
    Run {
        /// Directory containing scenarios
        #[arg(long)]
        directory: Option<std::path::PathBuf>,
        /// Pattern to match scenario names
        #[arg(long)]
        pattern: Option<String>,
        /// Run scenarios in parallel
        #[arg(long)]
        parallel: bool,
        /// Maximum number of parallel scenarios
        #[arg(long)]
        max_parallel: Option<usize>,
        /// Output file for results
        #[arg(long)]
        output_file: Option<std::path::PathBuf>,
        /// Generate detailed report
        #[arg(long)]
        detailed_report: bool,
    },
    /// Generate reports from scenario results
    Report {
        /// Input results file
        #[arg(long)]
        input: std::path::PathBuf,
        /// Output report file
        #[arg(long)]
        output: std::path::PathBuf,
        /// Report format (text, json, html)
        #[arg(long)]
        format: Option<String>,
        /// Include detailed information
        #[arg(long)]
        detailed: bool,
    },
}

/// CLI error types
#[derive(Debug, thiserror::Error)]
pub enum CliError {
    #[error("Command not found: {0}")]
    CommandNotFound(String),

    #[error("Invalid input: {0}")]
    InvalidInput(String),

    #[error("Configuration error: {0}")]
    Configuration(String),

    #[error("File system error: {0}")]
    FileSystem(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Not implemented: {0}")]
    NotImplemented(String),

    #[error("Authentication error: {0}")]
    Authentication(String),

    #[error("Network error: {0}")]
    Network(String),

    #[error("Operation failed: {0}")]
    OperationFailed(String),
}
